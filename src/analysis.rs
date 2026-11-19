use crate::model::Onset;
use rustfft::{FftPlanner,num_complex::Complex};

#[derive(Clone)]
struct Frame { time:f64,rms:f32,flux:f32,centroid:f32 }
pub struct Analysis { pub bpm:f64,pub onsets:Vec<Onset>,pub duration:f64 }
const RATE: f64=11_025.;
const FFT_N:usize=1024;
const HOP:usize=256;
fn median(mut v:Vec<f32>)->f32{
    if v.is_empty(){return 0.;}
    let middle=v.len()/2;
    v.select_nth_unstable_by(middle,|a,b|a.total_cmp(b));v[middle]
}
fn local_average(values:&[f32],radius:usize)->Vec<f32>{
    let mut pref=vec![0f64;values.len()+1];
    for (i,&x) in values.iter().enumerate(){pref[i+1]=pref[i]+x as f64;}
    (0..values.len()).map(|i|{
        let a=i.saturating_sub(radius);let b=(i+radius+1).min(values.len());
        ((pref[b]-pref[a])/(b-a) as f64) as f32
    }).collect()
}

pub fn analyze(mono:&[f32],mut progress:impl FnMut(f32))->Result<Analysis,String>{
    if mono.len()<FFT_N{return Err("Audio is too short for spectral analysis".into());}
    let total=(mono.len()-FFT_N)/HOP+1;
    let mut planner=FftPlanner::<f32>::new();let fft=planner.plan_fft_forward(FFT_N);
    let window=(0..FFT_N).map(|j|0.5-0.5*((j as f32)*std::f32::consts::TAU/FFT_N as f32).cos()).collect::<Vec<_>>();
    let mut scratch=vec![Complex{re:0f32,im:0f32};FFT_N];
    let mut prev=vec![0f32;FFT_N/2];let mut frames=Vec::with_capacity(total);
    for i in 0..total {
        let offset=i*HOP;
        let slice=&mono[offset..offset+FFT_N];
        let rms=(slice.iter().map(|&x|x*x).sum::<f32>()/FFT_N as f32).sqrt();
        for j in 0..FFT_N {scratch[j].re=slice[j]*window[j];scratch[j].im=0.;}
        fft.process(&mut scratch);
        let mut flux=0f32;let mut sum=0f32;let mut weighted=0f32;
        for k in 2..FFT_N/2 {
            let mag=scratch[k].norm();let bin_hz=k as f32*11025./FFT_N as f32;
            // Favor percussive mid-frequency flux without letting hiss dominate.
            let weight=if bin_hz<90. {0.35} else if bin_hz<3000.{1.0}else{0.58};
            flux+=(mag-prev[k]).max(0.)*weight;
            sum+=mag;weighted+=mag*bin_hz;
            prev[k]=mag;
        }
        frames.push(Frame{time:(offset+FFT_N/2) as f64/RATE,rms,flux,
            centroid:weighted/(sum.max(0.00001))});
        if i%900==0 {progress(i as f32/total as f32);}
    }
    let flux=frames.iter().map(|f|f.flux).collect::<Vec<_>>();
    let avg_flux=local_average(&flux,32);
    let rms=frames.iter().map(|f|f.rms).collect::<Vec<_>>();
    let short_rms=local_average(&rms,16);
    let median_rms=median(rms.iter().step_by(3).copied().collect());
    let smooth_flux=flux.iter().enumerate().map(|(i,&v)|{
        let median=avg_flux[i];
        (v/(median+0.002)).min(15.)*(v/8.0).sqrt().clamp(0.02,1.5)
    }).collect::<Vec<_>>();
    let strong=median(smooth_flux.iter().step_by(4).copied().collect());
    let floor=(strong*1.15).max(0.03);
    let mut candidate=Vec::new();
    let mut last=-100.0;
    for i in 2..frames.len().saturating_sub(2) {
        if smooth_flux[i]<floor || smooth_flux[i]<smooth_flux[i-1] || smooth_flux[i]<smooth_flux[i+1] {continue;}
        let power=(smooth_flux[i]/(strong+0.01)).clamp(0.,12.);
        let rel=(short_rms[i]/median_rms.max(0.000001)).max(0.00001);
        // Local musical-activity gate: genuine transients can survive even in a quiet intro.
        let quiet_penalty=if rel<0.33 {0.37}else if rel<0.55 {0.65}else{1.0};
        let strength=power*quiet_penalty;
        if strength<1.13 {continue;}
        let t=frames[i].time;
        if t-last<0.065 {
            if let Some(prev)=candidate.last_mut(){
                let p:&mut Onset=prev;
                if strength>p.strength {p.time=t;p.strength=strength;p.rms=frames[i].rms;p.centroid=frames[i].centroid;last=t;}
            }
            continue;
        }
        candidate.push(Onset{time:t,strength,rms:frames[i].rms,centroid:frames[i].centroid});last=t;
    }
    let duration=mono.len() as f64/RATE;
    let bpm=infer_bpm(&candidate);
    // Adapt density to 4/8-beat musical windows rather than a single song-wide count.
    let beat=60./bpm;
    let median_strength=median(candidate.iter().map(|x|x.strength).collect());
    let global_rms=median_rms.max(0.000001);
    let mut accepted=Vec::with_capacity(candidate.len());
    let mut left=0;
    for (i,onset) in candidate.iter().enumerate(){
        while left<i && candidate[left].time<onset.time-beat*8. {left+=1;}
        let recent=&candidate[left..=i];
        let local_rms=recent.iter().map(|x|x.rms).sum::<f32>()/recent.len() as f32;
        let ambient=(local_rms/global_rms).clamp(0.02,5.0);
        let density_gate=if ambient<0.32 {1.9}else if ambient<0.55{1.5}else{0.92};
        if onset.strength < median_strength*density_gate {continue;}
        if let Some(last)=accepted.last(){
            let prev:&Onset=last;
            if onset.time-prev.time<0.085 {continue;}
        }
        let mut snapped=onset.clone();
        // Conservative timing quantization to the detected sixteenth/eighth-note grid.
        let step=beat/4.;
        let rounded=(snapped.time/step).round()*step;
        if (snapped.time-rounded).abs()<0.018 {snapped.time=rounded;}
        accepted.push(snapped);
    }
    progress(1.);
    Ok(Analysis{bpm,onsets:accepted,duration})
}
fn infer_bpm(onsets:&[Onset])->f64{
    if onsets.len()<8{return 120.;}
    // Pairwise interval histogram, accounting for half-, quarter- and double-beat attacks.
    let mut bins=vec![0f32;181];
    for i in 0..onsets.len(){
        for j in i+1..(i+7).min(onsets.len()) {
            let dt=onsets[j].time-onsets[i].time;
            for subdiv in [0.25,0.5,1.0,2.0,4.0] {
                let bpm=(60.*subdiv/dt).round() as usize;
                if (65..=200).contains(&bpm){bins[bpm-65]+=onsets[i].strength.min(onsets[j].strength).min(2.)/(subdiv as f32).sqrt();}
            }
        }
    }
    let mut winner=(120,0f32);
    for (idx,&value) in bins.iter().enumerate(){
        let bpm=idx+65;
        let score=value+(if bpm>100 {0.06}else{0.0});
        if score>winner.1 {winner=(bpm,score);}
    }
    winner.0 as f64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn repeated_160_bpm_detected(){
        let notes=(0..400).map(|i|Onset{time:i as f64*0.375,strength:2.0,rms:0.2,centroid:900.}).collect::<Vec<_>>();
        assert!((infer_bpm(&notes)-160.).abs()<1.);
    }
}
