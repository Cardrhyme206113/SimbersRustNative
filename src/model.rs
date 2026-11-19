use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct Note {
    pub time: f64,
    pub x: u8,
    pub y: u8,
    pub hand: u8,
    pub dir: u8,
    #[serde(skip_serializing)]
    pub power: f32,
}
impl Note {
    pub fn mirror(&self) -> Self {
        let mirrored = [0, 1, 3, 2, 5, 4, 7, 6, 8];
        Self { x: 3 - self.x, hand: 1 - self.hand, dir: mirrored[self.dir as usize], ..self.clone() }
    }
}
#[derive(Clone, Debug)]
pub struct Obstacle { pub time: f64, pub duration: f64, pub x: u8 }
#[derive(Clone, Debug)]
pub struct Slider { pub head: Note, pub tail: Note }
#[derive(Clone, Debug)]
pub struct Chart {
    pub name: &'static str,
    pub rank: usize,
    pub characteristic: &'static str,
    pub notes: Vec<Note>,
    pub walls: Vec<Obstacle>,
    pub arcs: Vec<Slider>,
}
#[derive(Clone)]
pub struct Project {
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub bpm: f64,
    pub charts: Vec<Chart>,
    pub audio_ogg: Arc<Vec<u8>>,
    pub analysis_onsets: usize,
    pub profile: Vec<(String, f64)>,
}
#[derive(Clone, Debug)]
pub struct Options {
    pub intensity: f32,
    pub arcs: bool,
    pub walls: bool,
    pub bombs: bool,
    pub bot_review: bool,
    pub highlight_minutes: Option<u32>,
    pub vorbis_quality: f32,
}
impl Default for Options {
    fn default() -> Self { Self {
        intensity: 1.0, arcs: true, walls: true, bombs: true, bot_review: true,
        highlight_minutes: None, vorbis_quality: 0.32,
    }}
}
#[derive(Clone, Debug)]
pub struct Onset { pub time:f64, pub strength:f32, pub rms:f32, pub centroid:f32 }
pub const ANGLES: [i32; 9] = [0,180,270,90,315,45,225,135,180];
pub const OPPOSITE:[u8;9] = [1,0,3,2,7,6,5,4,1];
pub fn angle_diff(a:u8,b:u8)->i32{
    let diff=(ANGLES[a.min(8) as usize]-ANGLES[b.min(8) as usize]).abs()%360;
    diff.min(360-diff)
}
pub fn is_human_return(a:u8,b:u8)->bool { a<8 && b<8 && angle_diff(a,b)>=135 }
