use crate::model::{angle_diff, is_human_return};

// Direct copy of Simbers7's 121,106-record, human-derived corpus.
// Each record is [length, popularity, wildness, note-byte * length].
// Notes are [direction:4, row:2, column:2]. Included in the .exe at compile time.
static RAW: &[u8] = include_bytes!("../assets/human-flow-corpus.bin");
#[derive(Clone, Copy, Debug)]
pub struct Gesture { pub x:u8,pub y:u8,pub d:u8 }
#[derive(Clone)]
pub struct Phrase { pub notes: Vec<Gesture>,pub support:f32,pub ease:f32,pub id:usize }
impl Phrase {
    pub fn mirrored(&self)-> Self {
        let mut p=self.clone();
        for n in &mut p.notes {
            n.x=3-n.x;
            n.d= [0,1,3,2,5,4,7,6,8][n.d as usize];
        }
        p
    }
}
pub fn load()->Vec<Phrase>{
    let mut out=Vec::with_capacity(20000);
    let mut p=0;
    while p+3<RAW.len() {
        let start=p; let len=RAW[p] as usize;
        let popularity=RAW[p+1];let wild=RAW[p+2];p+=3;
        if !(3..=10).contains(&len) || p+len>RAW.len(){break;}
        let mut notes=Vec::with_capacity(len);
        for &v in &RAW[p..p+len] {
            notes.push(Gesture{x:v&3,y:(v>>2)&3,d:v>>4});
        }
        p+=len;
        if notes.iter().any(|n|n.y>2 || n.d>7 || n.d==2 || n.d==3){continue;}
        if notes.windows(2).any(|w|!is_human_return(w[0].d,w[1].d)){continue;}
        let lateral=notes.windows(2).map(|w|w[0].x.abs_diff(w[1].x) as f32).sum::<f32>();
        let diagonal=notes.iter().filter(|n|n.d>=4).count() as f32;
        let across=notes.windows(2).map(|w|angle_diff(w[0].d,w[1].d)).sum::<i32>();
        let ease=(lateral*0.8+diagonal*1.3+(wild as f32)/130.0)/(len as f32)
            -(across as f32)/(len as f32)*0.0002;
        out.push(Phrase{notes,support:popularity as f32/255.0,ease,id:start});
    }
    // Small templates don't lose all phrase repetition due to insignificant lane changes.
    out.sort_by(|a,b|a.ease.total_cmp(&b.ease).then_with(||b.support.total_cmp(&a.support)));
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn corpus_available(){let corpus=load();assert!(corpus.len()>1000,"Human phrase corpus should contain thousands of safe phrases");}
    #[test]fn corpus_is_consistent(){assert_eq!(RAW.len(),1_155_194);}
}
