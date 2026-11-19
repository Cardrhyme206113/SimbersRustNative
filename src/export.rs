use crate::{audio::{self,DecodedAudio},model::*};
use std::{io::{Cursor,Write},fs::File,path::Path};
use serde_json::{json,Value};
use ogg::{PacketWriter,writing::PacketWriteEndInfo};
use zip::{ZipWriter,write::FileOptions,CompressionMethod};

const COVER:&[u8]=include_bytes!("../assets/cover.jpg");
fn beat(t:f64,bpm:f64)->f64 {(t*bpm/60.*10000.).round()/10000.}
fn chart_json(chart:&Chart,bpm:f64)->Value{
    let notes=chart.notes.iter().map(|n|json!({
        "_time":beat(n.time,bpm),"_lineIndex":n.x,"_lineLayer":n.y,
        "_type":n.hand,"_cutDirection":n.dir,
    })).collect::<Vec<_>>();
    let mut events=Vec::new();
    if chart.characteristic=="90Degree" || chart.characteristic=="360Degree" {
        let full=chart.characteristic=="360Degree";
        for (i,n) in chart.notes.iter().step_by(36).enumerate(){
            events.push(json!({"_time":beat(n.time,bpm),"_type":15,
                "_value":if full {if i%2==0{2}else{3}}else{if i%2==0{0}else{1}}}));
        }
    }
    let walls=chart.walls.iter().map(|wall|json!({
        "_time":beat(wall.time,bpm),"_duration":beat(wall.duration,bpm),
        "_lineIndex":wall.x,"_type":0,"_width":1,
    })).collect::<Vec<_>>();
    let arcs=chart.arcs.iter().map(|arc|json!({
        "_colorType":arc.head.hand,
        "_headTime":beat(arc.head.time,bpm),"_headLineIndex":arc.head.x,
        "_headLineLayer":arc.head.y,"_headCutDirection":arc.head.dir,
        "_headControlPointLengthMultiplier":1.0,
        "_tailTime":beat(arc.tail.time,bpm),"_tailLineIndex":arc.tail.x,
        "_tailLineLayer":arc.tail.y,"_tailCutDirection":arc.tail.dir,
        "_tailControlPointLengthMultiplier":1.0,"_sliderMidAnchorMode":1,
    })).collect::<Vec<_>>();
    json!({"_version":"2.6.0","_notes":notes,"_obstacles":walls,
        "_events":events,"_sliders":arcs,"_waypoints":[],
        "_customData":{"_generator":"Simbers Native / Rust","_humanFlow":true}})
}
fn title_slug(text:&str)->String{
    text.chars().map(|c| if c.is_ascii_alphanumeric()||c=='-' {c}else{'_'})
    .take(80).collect()
}
fn chart_filename(chart:&Chart)->String{
    format!("{}{}.dat",chart.name,chart.characteristic)
}
pub fn export_zip(p:&Project,dest:&Path)->Result<(),String>{
    let mut zip=ZipWriter::new(File::create(dest).map_err(|e|e.to_string())?);
    let opts=FileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("song.ogg",opts).map_err(|e|e.to_string())?;
    zip.write_all(&p.audio_ogg).map_err(|e|e.to_string())?;
    zip.start_file("cover.jpg",opts).map_err(|e|e.to_string())?;
    zip.write_all(COVER).map_err(|e|e.to_string())?;
    let json_opts=FileOptions::default().compression_method(CompressionMethod::Deflated);
    for chart in &p.charts {
        zip.start_file(chart_filename(chart),json_opts).map_err(|e|e.to_string())?;
        let value=chart_json(chart,p.bpm);
        serde_json::to_writer(&mut zip,&value).map_err(|e|e.to_string())?;
    }
    let char_sets=["Standard","NoArrows","OneSaber","90Degree","360Degree","Lawless"]
        .iter().map(|characteristic|{
            let beatmaps=p.charts.iter().filter(|c|c.characteristic==*characteristic)
                .map(|c|json!({
                    "_difficulty":c.name,"_difficultyRank":c.rank+1,
                    "_beatmapFilename":chart_filename(c),
                    "_noteJumpMovementSpeed": 10.0+c.rank as f32*1.8,
                    "_noteJumpStartBeatOffset":0.0,"_customData":{},
                })).collect::<Vec<_>>();
            json!({"_beatmapCharacteristicName":characteristic,"_difficultyBeatmaps":beatmaps})
        }).collect::<Vec<_>>();
    let info=json!({"_version":"2.0.0","_songName":p.title,
        "_songSubName":"","_songAuthorName":p.artist,"_levelAuthorName":"Simbers Native",
        "_beatsPerMinute":p.bpm,"_songTimeOffset":0.0,"_shuffle":0,"_shufflePeriod":0.5,
        "_previewStartTime":0,"_previewDuration":10,
        "_songFilename":"song.ogg","_coverImageFilename":"cover.jpg",
        "_environmentName":"DefaultEnvironment","_allDirectionsEnvironmentName":"GlassDesertEnvironment",
        "_difficultyBeatmapSets":char_sets,"_customData":{}});
    zip.start_file("Info.dat",json_opts).map_err(|e|e.to_string())?;
    serde_json::to_writer(&mut zip,&info).map_err(|e|e.to_string())?;
    zip.finish().map_err(|e|e.to_string())?;
    Ok(())
}
pub fn vorbis_encode(audio:&DecodedAudio,quality:f32)->Result<Vec<u8>,String>{
    // Pure Rust: Symphonia (decode), Rust Vorbis (encode), ogg (mux).
    let out_rate=32_000;
    let pcm=audio::resample_interleaved(&audio.interleaved,audio.channels,audio.sample_rate,out_rate);
    let stereo=if audio.channels==1 {
        let mut buffer=Vec::with_capacity(pcm.len()*2);
        for s in pcm {buffer.extend_from_slice(&[s,s]);}
        buffer
    } else {pcm};
    let cfg=rusty_vorbis::VorbisEncoderConfig{bitrate_bps:128_000,quality:quality.clamp(0.05,0.98)};
    let mut encoder=rusty_vorbis::VorbisEncoder::new(cfg);
    encoder.push_pcm_f32(&stereo,2,out_rate).map_err(|e|format!("Vorbis input: {e:?}"))?;
    encoder.finish();
    let mut packets=Vec::new();
    loop {
        match encoder.next_packet() {
            Ok(pkt)=>packets.push(pkt),
            Err(rusty_vorbis::Error::Eof)=>break,
            Err(e)=>return Err(format!("Vorbis encode: {e:?}")),
        }
    }
    if packets.len()<4{return Err("Vorbis encoder returned no audio packets".into());}
    let mut ogg=PacketWriter::new(Vec::new());
    let total=packets.len();
    for (i,p) in packets.into_iter().enumerate(){
        let end=if i+1==total{PacketWriteEndInfo::EndStream}
            else if i==0{PacketWriteEndInfo::EndPage}
            else{PacketWriteEndInfo::NormalPacket};
        ogg.write_packet(p.data,0x5b37b5a7,end,p.pts.max(0) as u64)
            .map_err(|e|format!("Ogg mux: {e}"))?;
    }
    let data=ogg.into_inner();
    if !data.starts_with(b"OggS") {return Err("Ogg header missing".into());}
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn no_full_width_walls(){
        let chart=Chart{name:"ExpertPlus",rank:4,characteristic:"Standard",notes:Vec::new(),
            walls:vec![Obstacle{time:2.,duration:0.3,x:3}],arcs:Vec::new()};
        let v=chart_json(&chart,120.);
        assert_eq!(v["_obstacles"][0]["_width"],1);
    }
}
