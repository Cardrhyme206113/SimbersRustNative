use std::{fs::File, io::BufReader, path::Path};
use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, errors::Error as AudioError,
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};

pub struct DecodedAudio {
    pub sample_rate:u32,
    pub channels:usize,
    pub interleaved:Vec<f32>,
    pub mono_analysis:Vec<f32>,
    pub duration:f64,
}

pub fn decode(path:&Path, duration_limit:Option<f64>, mut progress:impl FnMut(f32))->Result<DecodedAudio,String>{
    let file=File::open(path).map_err(|e|e.to_string())?;
    let media=MediaSourceStream::new(Box::new(BufReader::new(file)),Default::default());
    let mut hint=Hint::new();
    if let Some(ext)=path.extension().and_then(|s|s.to_str()){hint.with_extension(ext);}
    let probed=symphonia::default::get_probe().format(&hint,media,&FormatOptions::default(),&MetadataOptions::default())
        .map_err(|e|format!("Could not identify audio: {e}"))?;
    let mut format=probed.format;
    let track=format.default_track().ok_or("No playable audio track")?;
    let track_id=track.id;
    let expected_frames=track.codec_params.n_frames.unwrap_or(0);
    let mut decoder=symphonia::default::get_codecs().make(&track.codec_params,&DecoderOptions::default())
        .map_err(|e|format!("Unsupported codec: {e}"))?;
    let mut rate=track.codec_params.sample_rate.unwrap_or(0);
    let mut channels=track.codec_params.channels.map(|x|x.count()).unwrap_or(0);
    let mut pcm=Vec::new();
    let mut packets=0u64;
    loop {
        let packet=match format.next_packet(){
            Ok(p)=>p,
            Err(AudioError::IoError(e)) if e.kind()==std::io::ErrorKind::UnexpectedEof =>break,
            Err(AudioError::ResetRequired)=>return Err("Mid-stream codec change is unsupported".to_string()),
            Err(e)=>return Err(format!("Audio packet error: {e}")),
        };
        if packet.track_id()!=track_id {continue;}
        let decoded=match decoder.decode(&packet){
            Ok(a)=>a,
            Err(AudioError::DecodeError(_))=>continue,
            Err(AudioError::IoError(e)) if e.kind()==std::io::ErrorKind::UnexpectedEof=>break,
            Err(e)=>return Err(format!("Audio decode error: {e}")),
        };
        let spec=*decoded.spec();
        let ch=spec.channels.count();
        if channels!=0 && channels!=ch {return Err("Changing channel counts are unsupported".into());}
        if rate!=0 && rate!=spec.rate {return Err("Changing sample rate is unsupported".into());}
        channels=ch;rate=spec.rate;
        let mut buf=SampleBuffer::<f32>::new(decoded.capacity() as u64,spec);
        buf.copy_interleaved_ref(decoded);
        let samples=buf.samples();
        if channels>2 {
            for frame in samples.chunks(channels){
                let mono=frame.iter().sum::<f32>()/channels as f32;
                pcm.extend_from_slice(&[mono,mono]);
            }
        } else {
            pcm.extend_from_slice(samples);
        }
        packets+=1;
        if packets%120==0 {
            let position=pcm.len() as f64/channels.min(2).max(1) as f64;
            if expected_frames>0 { progress((position/expected_frames as f64).min(0.95) as f32); }
        }
        if let Some(limit)=duration_limit {
            let frames=pcm.len()/(channels.min(2).max(1));
            if rate>0 && frames as f64 / rate as f64 >= limit {break;}
        }
    }
    if pcm.is_empty() || rate==0 {return Err("Audio decoder returned no samples".into());}
    let out_channels=channels.min(2).max(1);
    let length=pcm.len()/out_channels;
    let duration=length as f64/rate as f64;
    let mono=pcm.chunks(out_channels).map(|x|x.iter().sum::<f32>()/x.len() as f32).collect::<Vec<_>>();
    let mono_analysis=resample(&mono,rate,11_025);
    Ok(DecodedAudio{sample_rate:rate,channels:out_channels,interleaved:pcm,mono_analysis,duration})
}

pub fn resample(signal:&[f32],from:u32,to:u32)->Vec<f32>{
    if from==to{return signal.to_vec();}
    let n=((signal.len() as f64*to as f64/from as f64).round() as usize).max(1);
    let ratio=from as f64/to as f64;
    let mut out=Vec::with_capacity(n);
    for i in 0..n {
        let at=i as f64*ratio;
        let a=(at as usize).min(signal.len().saturating_sub(1));
        let b=(a+1).min(signal.len().saturating_sub(1));
        out.push(signal[a]+(signal[b]-signal[a])*(at-a as f64) as f32);
    }
    out
}

pub fn resample_interleaved(signal:&[f32],channels:usize,from:u32,to:u32)->Vec<f32>{
    if from==to{return signal.to_vec();}
    let frames=signal.len()/channels;
    let n=((frames as f64*to as f64/from as f64).round() as usize).max(1);
    let ratio=from as f64/to as f64;
    let mut out=Vec::with_capacity(n*channels);
    for i in 0..n {
        let at=i as f64*ratio;
        let a=(at as usize).min(frames.saturating_sub(1));
        let b=(a+1).min(frames.saturating_sub(1));
        let mix=(at-a as f64) as f32;
        for c in 0..channels{let x=signal[a*channels+c];out.push(x+(signal[b*channels+c]-x)*mix);}
    }
    out
}
