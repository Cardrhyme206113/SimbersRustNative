use crate::{analysis::{self,Analysis},audio,corpus::{self,Phrase,Gesture},model::*};
use std::{collections::{HashMap,HashSet},path::Path,sync::Arc,time::Instant};

const LEVELS:[(&str,f32,f32);5]=[
    ("Easy",0.2,2.5),("Normal",0.4,3.5),("Hard",0.6,5.0),
    ("Expert",0.8,7.0),("ExpertPlus",1.0,9.0)
];
const MODES:[&str;6]=["Standard","NoArrows","OneSaber","90Degree","360Degree","Lawless"];

fn hash_u64(mut x:u64)->u64{
    x^=x>>30;x=x.wrapping_mul(0xbf58476d1ce4e5b9);
    x^=x>>27;x=x.wrapping_mul(0x94d049bb133111eb);x^(x>>31)
}
fn phrase_key(events:&[Onset])->u64{
    let mut key=0x9e3779b97f4a7c15u64;
    for pair in events.windows(2) {
        let unit=((pair[1].time-pair[0].time)*100.0).round() as i64;
        key=hash_u64(key^unit.max(0) as u64);
    }
    key=hash_u64(key^((events.iter().map(|x|x.strength).sum::<f32>()/events.len().max(1) as f32*8.) as u64));
    key
}
fn quality(n:&Note,prev:Option<&Note>,t:f64,density:f32,variant:usize)->f32{
    let Some(prev)=prev else {return 0.};
    if !is_human_return(prev.dir,n.dir){return 1e6;}
    let dt=(t-prev.time).max(0.055) as f32;
    let dx=n.x as f32-prev.x as f32;
    let dy=n.y as f32-prev.y as f32;
    let dist=(dx*dx+dy*dy).sqrt();
    let wrist= if angle_diff(prev.dir,n.dir)<160 {0.14}else{0.};
    let conservative=0.9+variant as f32*0.062;
    let lateral=if density>6.5 {2.1}else{0.7};
    ((dist*conservative/dt).max(0.0)-10.0).max(0.)*0.85
        +lateral*dx.abs()*(if dt<0.16 {1.4}else{0.5})+wrist
}

fn pool()->Vec<Phrase>{
    let lib=corpus::load();let mut patterns=Vec::new();let mut seen=HashSet::new();
    for p in lib.into_iter().filter(|p|p.notes.len()==4){
        let key=p.notes.iter().fold(0u64,|k,n|(k<<8)|((n.d as u64)<<4)|((n.y as u64)<<2)|(n.x as u64));
        // Prioritize viable, high-support motifs, but avoid identical copies.
        if !seen.insert(key){continue;}
        patterns.push(p);
        if patterns.len()>=256 {break;}
    }
    patterns
}
fn select_onsets(analysis:&Analysis, fraction:f32, max_nps:f32, intensity:f32)->Vec<Onset>{
    let a=&analysis.onsets;
    if a.is_empty(){return Vec::new();}
    let frac=(fraction*intensity).clamp(0.05,1.0);
    let target=(a.len() as f32*frac).round().max(1.) as usize;
    let mut sorted=(0..a.len()).collect::<Vec<_>>();
    // Slightly preserve rhythmic events near grid and recognizable phrase boundaries.
    sorted.sort_by(|&i,&j|{
        let tick=60./analysis.bpm/4.;
        let offset=|t:f64| ((t/tick).round()*tick-t).abs();
        let score=|index:usize| a[index].strength+
            if offset(a[index].time)<0.013 {0.2}else{0.};
        score(j).total_cmp(&score(i))
    });
    sorted.truncate(target);sorted.sort_unstable();
    let mut kept=Vec::new();
    for index in sorted {
        let onset=&a[index];
        let window=kept.iter().rev().take_while(|o:&&Onset| onset.time-o.time < 4.0).count();
        if window as f32 >= max_nps*4.0 {continue;}
        if kept.last().is_some_and(|o:&Onset|onset.time-o.time<0.095){continue;}
        kept.push(onset.clone());
    }
    kept
}

fn find_best_phrase(pool:&[Phrase],group:&[Onset],hand:u8,last:Option<&Note>,
                    reuse:Option<usize>,bot:bool,offset:usize)->(Vec<Note>,usize){
    let density=(group.len() as f32/((group.last().unwrap().time-group[0].time).max(0.20) as f32)).min(20.);
    let mut winner=None;
    let mut indices=(0..pool.len().min(96)).step_by(1).collect::<Vec<_>>();
    // Reserve the repeated macro's existing phrase even if it is not in the first 96.
    if let Some(i)=reuse {if i<pool.len() && !indices.contains(&i){indices.push(i);}}
    for &idx in &indices {
        let phrase=&pool[idx];
        let source:Vec<Gesture>=if hand==1 {phrase.mirrored().notes}else{phrase.notes.clone()};
        for flip in [false,true]{
            let mut notes=Vec::new();let mut prior=last.cloned();let mut risk=0.;
            for (i,&step) in source.iter().enumerate().take(group.len()){
                let d=if flip{OPPOSITE[step.d as usize]}else{step.d};
                let x=if hand==0{step.x%2}else{2+(step.x%2)};
                let note=Note{time:group[i].time,x,y:step.y,hand,dir:d,power:group[i].strength};
                if prior.as_ref().is_some_and(|p|!is_human_return(p.dir,d)) {risk=1e7;break;}
                if bot {
                    for v in 0..8 {risk+=quality(&note,prior.as_ref(),note.time,density,v)*0.125;}
                } else {risk+=quality(&note,prior.as_ref(),note.time,density,0);}
                prior=Some(note.clone());notes.push(note);
            }
            if notes.len()==group.len(){
                let lateral=notes.windows(2).map(|w|w[0].x.abs_diff(w[1].x) as f32).sum::<f32>();
                let diagonal=notes.iter().filter(|n|n.dir>=4).count() as f32;
                let score=risk+phrase.ease*0.4 +
                    (if density>6.0{2.2}else{0.25})*diagonal+
                    (if density>6.0{2.8}else{0.5})*lateral+
                    (if Some(idx)==reuse{-2.7}else{0.})+
                    (((idx+offset*13)%97) as f32)*0.002;
                if winner.as_ref().is_none_or(|w:&(f32,Vec<Note>,usize)|score<w.0) {winner=Some((score,notes.clone(),idx));}
            }
        }
    }
    if let Some((_,ns,i))=winner{return (ns,i);}
    // Guaranteed playable two-direction return if no corpus entry fits the boundary.
    let mut fallback=Vec::new();let mut d=last.map(|n|OPPOSITE[n.dir as usize]).unwrap_or(if hand==0{0}else{1});
    for (i,onset) in group.iter().enumerate(){
        fallback.push(Note{time:onset.time,x:if hand==0{(i%2) as u8}else{(2+i%2) as u8},
            y:if d==0{2}else{0},hand,dir:d,power:onset.strength});
        d=OPPOSITE[d as usize];
    }
    (fallback,0)
}
fn build_notes(a:&Analysis,opts:&Options,rank:usize,pool:&[Phrase])->Vec<Note>{
    let (_,fraction,max_nps)=LEVELS[rank];
    let selected=select_onsets(a,fraction,max_nps,opts.intensity);
    let mut notes=Vec::with_capacity(selected.len()*2);
    let mut reused=HashMap::<u64,usize>::new();
    let mut last=[None::<Note>,None::<Note>];
    for (block,group) in selected.chunks(4).enumerate(){
        let key=phrase_key(group);
        let hand=(block%2) as u8;
        let (chosen,id)=find_best_phrase(pool,group,hand,last[hand as usize].as_ref(),
            reused.get(&key).copied(),opts.bot_review,block);
        reused.insert(key,id);
        if let Some(tail)=chosen.last(){last[hand as usize]=Some(tail.clone());}
        notes.extend(chosen);
    }
    // Occasional double accents; maintain safe two-sided lanes and independent hand return.
    let mut with_double=Vec::with_capacity(notes.len()*2);
    let mut double_last=[None::<Note>,None::<Note>];
    for n in notes {
        let hand=n.hand as usize;
        double_last[hand]=Some(n.clone());
        let make_double=(n.power>3.1 && n.time>0.15 &&
            hash_u64((n.time*1000.) as u64)%11==0) && rank>=2;
        if make_double {
            let mut partner=n.mirror();
            let other=partner.hand as usize;
            let prev=double_last[other].as_ref();
            if let Some(prev)=prev {if !is_human_return(prev.dir,partner.dir){partner.dir=OPPOSITE[prev.dir as usize];}}
            // Two simultaneous swings in separate halves, never in the same column.
            if partner.x==n.x {partner.x=if n.hand==0{3}else{0};}
            with_double.push(partner.clone());double_last[other]=Some(partner);
        }
        with_double.push(n);
    }
    with_double.sort_by(|a,b|a.time.total_cmp(&b.time).then_with(||a.hand.cmp(&b.hand)));
    // Density culling is deliberately hand-neutral, never red-first.
    let mut result=Vec::with_capacity(with_double.len());
    let mut last_time=[-100f64,-100f64];
    for n in with_double {
        if n.time-last_time[n.hand as usize]<0.085 {continue;}
        last_time[n.hand as usize]=n.time;
        result.push(n);
    }
    // Parity check of real emitted sequences; do not change timestamps, only directions.
    for hand in 0..=1 {
        let mut last:Option<u8>=None;
        for n in result.iter_mut().filter(|n|n.hand==hand){
            if last.is_some_and(|p|!is_human_return(p,n.dir)){
                n.dir=OPPOSITE[last.unwrap() as usize];
            }
            last=Some(n.dir);
        }
    }
    result
}

fn make_arcs(notes:&[Note])->Vec<Slider>{
    let mut out=Vec::new();let mut last:[Option<&Note>;2]=[None,None];
    for (i,n) in notes.iter().enumerate(){
        let old=last[n.hand as usize];
        if i%7==0 {
            if let Some(p)=old {
                let dt=n.time-p.time;
                if (0.15..1.0).contains(&dt) && is_human_return(p.dir,n.dir){
                    out.push(Slider{head:p.clone(),tail:n.clone()});
                }
            }
        }
        last[n.hand as usize]=Some(n);
    }
    out
}
fn make_walls(notes:&[Note],duration:f64)->Vec<Obstacle>{
    let mut walls=Vec::new();let mut t=5.0;
    let mut side=0u8;
    while t<duration-2.0 {
        if notes.iter().all(|n| (n.time-t).abs()>0.30 || n.x!=side){
            walls.push(Obstacle{time:t,duration:0.5,x:side});side=3-side;
        }
        t+=7.0;
    }
    walls
}
fn one_saber(chart:&Chart)->Chart{
    let mut c=chart.clone();c.characteristic="OneSaber";
    c.notes.retain(|n|n.hand==0);
    c.arcs.retain(|a|a.head.hand==0 && a.tail.hand==0);
    c
}
fn ensure_return(notes:&mut [Note]){
    for h in 0..2 {
        let mut last=None;
        for n in notes.iter_mut().filter(|n|n.hand==h){
            if let Some(p)=last{if !is_human_return(p,n.dir){n.dir=OPPOSITE[p as usize];}}
            last=Some(n.dir);
        }
    }
}
pub fn generate(path:&Path,opts:&Options,mut update:impl FnMut(f32,&str))->Result<Project,String>{
    let started=Instant::now();let mut profile=Vec::new();
    update(0.01,"Decoding audio (Symphonia)");
    let decode_start=Instant::now();
    let audio=audio::decode(path,opts.highlight_minutes.map(|x|x as f64*60.),|p|update(0.01+p*0.13,"Decoding audio"))?;
    profile.push(("Native audio decode".into(),decode_start.elapsed().as_secs_f64()*1000.));
    update(0.16,"FFT and onset detection (RustFFT)");
    let stage=Instant::now();
    let analysis=analysis::analyze(&audio.mono_analysis,|p|update(0.16+p*0.36,"Spectral analysis and activity gating"))?;
    profile.push(("FFT and beat analysis".into(),stage.elapsed().as_secs_f64()*1000.));
    let stage=Instant::now();update(0.53,"Loading human choreography corpus");
    let corpus=pool();if corpus.is_empty(){return Err("No playable phrases in the bundled corpus".into());}
    profile.push(("Human corpus indexing".into(),stage.elapsed().as_secs_f64()*1000.));
    let stage=Instant::now();let mut charts=Vec::new();
    for (r,(difficulty,_,_)) in LEVELS.iter().enumerate(){
        update(0.56+r as f32*0.035,"Planning repeated 4-note phrases and validating hand flow");
        let notes=build_notes(&analysis,opts,r,&corpus);
        let standard=Chart{name:difficulty,rank:r,characteristic:"Standard",
            walls:if opts.walls{make_walls(&notes,audio.duration)}else{Vec::new()},
            arcs:if opts.arcs{make_arcs(&notes)}else{Vec::new()},notes};
        charts.push(standard.clone());
        for mode in MODES.into_iter().skip(1){
            let mut variant=if mode=="OneSaber"{one_saber(&standard)}else{standard.clone()};
            variant.characteristic=mode;
            if mode=="NoArrows"{for n in &mut variant.notes{n.dir=8;}variant.arcs.clear();}
            if mode=="Lawless"{
                // Alter only the intended Lawless variation; Standard stays parity-safe.
                for (i,n) in variant.notes.iter_mut().enumerate(){if i%13==0 {n.dir=if n.dir<8{OPPOSITE[n.dir as usize]}else{8};}}
            }
            if mode!="Lawless" && mode!="NoArrows" {ensure_return(&mut variant.notes);}
            charts.push(variant);
        }
    }
    profile.push(("Choreography + 8-variant scoring".into(),stage.elapsed().as_secs_f64()*1000.));
    update(0.76,"Encoding Ogg Vorbis (native Rust)");
    let stage=Instant::now();
    let ogg=crate::export::vorbis_encode(&audio,opts.vorbis_quality)?;
    profile.push(("Rust Vorbis encoding".into(),stage.elapsed().as_secs_f64()*1000.));
    let file_stem=path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let title=file_stem.split(" - ").last().unwrap_or(&file_stem).trim().to_string();
    let artist=file_stem.split(" - ").next().unwrap_or("Unknown Artist").trim().to_string();
    profile.push(("Total generation".into(),started.elapsed().as_secs_f64()*1000.));
    update(1.0,"Ready — all maps generated");
    Ok(Project{title,artist,duration:audio.duration,bpm:analysis.bpm,
        charts,audio_ogg:Arc::new(ogg),analysis_onsets:analysis.onsets.len(),profile})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn every_note_returns(){
        let mut n=vec![Note{time:0.,x:0,y:0,hand:0,dir:0,power:1.},
                        Note{time:1.,x:0,y:1,hand:0,dir:0,power:1.}];
        ensure_return(&mut n);assert_eq!(n[1].dir,1);
    }
    #[test]fn all_characteristics(){assert_eq!(MODES.len(),6);assert_eq!(LEVELS.len(),5);}
}
