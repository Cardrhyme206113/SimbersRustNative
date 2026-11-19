use crate::{engine, export,model::{Options,Project,Chart,Note}};
use eframe::egui::{self,Align2,Color32,FontId,Pos2,Rect,Stroke,Vec2};
use rodio::{Decoder,OutputStream,OutputStreamHandle,Sink};
use std::{fs::File,io::BufReader,path::{Path,PathBuf},sync::{mpsc::{self,Receiver,Sender},Arc},thread,time::{Duration,Instant}};

const PANEL:Color32=Color32::from_rgb(29,29,29);
const BLUE:Color32=Color32::from_rgb(0,123,255);
const MUTED:Color32=Color32::from_rgb(150,150,150);
const FG:Color32=Color32::from_rgb(244,244,244);

enum Update {Progress(f32,String),Generated(Result<Project,String>),Exported(Result<PathBuf,String>)}
struct Player {
    _stream:Option<OutputStream>,handle:Option<OutputStreamHandle>,sink:Option<Sink>,
    playing:bool,pos:f32,
}
impl Player {
    fn new()->Self{Self{_stream:None,handle:None,sink:None,playing:false,pos:0.}}
    fn load(&mut self,file:&Path)->Result<(),String>{
        self.sink=None;self.playing=false;self.pos=0.;
        if self.handle.is_none(){
            let (stream,handle)=OutputStream::try_default().map_err(|e|e.to_string())?;
            self._stream=Some(stream);self.handle=Some(handle);
        }
        let decoder=Decoder::new(BufReader::new(File::open(file).map_err(|e|e.to_string())?))
            .map_err(|e|e.to_string())?;
        let sink=Sink::try_new(self.handle.as_ref().unwrap()).map_err(|e|e.to_string())?;
        sink.append(decoder);sink.pause();self.sink=Some(sink);Ok(())
    }
    fn toggle(&mut self){
        if let Some(s)=&self.sink {
            if self.playing{s.pause();}else{s.play();}
            self.playing=!self.playing;
        }
    }
    fn seek(&mut self,time:f32){
        self.pos=time.max(0.);
        if let Some(s)=&self.sink {let _=s.try_seek(Duration::from_secs_f32(self.pos));}
    }
    fn current(&self)->f32 {self.sink.as_ref().map_or(self.pos,|s|s.get_pos().as_secs_f32())}
}

pub struct App {
    options:Options,
    source:Option<PathBuf>,
    project:Option<Arc<Project>>,
    selected_chart:usize,
    busy:bool,
    progress:f32,
    status:String,
    sender:Sender<Update>,receiver:Receiver<Update>,
    player:Player,
    max_preview:usize,
}
impl Default for App {
    fn default()->Self {
        let (sender,receiver)=mpsc::channel();
        Self{options:Options::default(),source:None,project:None,selected_chart:0,
            busy:false,progress:0.,status:"Choose a local audio file".into(),
            sender,receiver,player:Player::new(),max_preview:3000}
    }
}
fn button(ui:&mut egui::Ui,text:&str,enabled:bool)->egui::Response {
    ui.add_enabled_ui(enabled,|ui|ui.add_sized([ui.available_width(),38.],
        egui::Button::new(egui::RichText::new(text).color(FG).strong())
            .fill(if enabled{BLUE}else{Color32::from_rgb(55,55,55)})))
        .inner
}
fn name_of(p:&Path)->String{p.file_name().unwrap_or_default().to_string_lossy().to_string()}
impl App {
    fn browse(&mut self){
        let chosen=rfd::FileDialog::new().add_filter("Audio",&["mp3","ogg","flac","wav","m4a","aac"])
            .pick_file();
        if let Some(path)=chosen {
            self.status=format!("Loaded {}",name_of(&path));
            self.player=Player::new();
            if let Err(e)=self.player.load(&path) {self.status=format!("Loaded audio (preview unavailable: {e})");}
            self.source=Some(path);self.project=None;self.progress=0.;
        }
    }
    fn generate(&mut self,ctx:&egui::Context){
        let Some(path)=self.source.clone() else{return;};
        self.busy=true;self.progress=0.0;self.status="Preparing generation".into();
        let opts=self.options.clone();let tx=self.sender.clone();let repaint=ctx.clone();
        thread::spawn(move||{
            let result=engine::generate(&path,&opts,|v,text|{
                let _=tx.send(Update::Progress(v,text.to_string()));
                repaint.request_repaint();
            });
            let _=tx.send(Update::Generated(result));repaint.request_repaint();
        });
    }
    fn export(&mut self,ctx:&egui::Context){
        let Some(p)=self.project.clone() else{return;};
        let name=p.title.chars().map(|c|if c.is_ascii_alphanumeric()||c=='-'||c==' ' {c}else{'_'}).collect::<String>();
        let destination=rfd::FileDialog::new().set_file_name(format!("{name}_SimbersNative.zip"))
            .add_filter("Beat Saber level ZIP", &["zip"]).save_file();
        if let Some(path)=destination {
            self.busy=true;self.status="Exporting ZIP".into();let tx=self.sender.clone();let repaint=ctx.clone();
            thread::spawn(move||{
                let result=export::export_zip(&p,&path).map(|_|path);
                let _=tx.send(Update::Exported(result));repaint.request_repaint();
            });
        }
    }
    fn poll(&mut self){
        while let Ok(update)=self.receiver.try_recv(){match update {
            Update::Progress(p,s)=>{self.progress=p;self.status=s;}
            Update::Generated(Ok(p))=>{
                self.project=Some(Arc::new(p));self.progress=1.;self.status="Generation complete".into();
                self.selected_chart=4*6;self.busy=false;self.player.seek(0.);
            }
            Update::Generated(Err(e))=>{self.busy=false;self.status=format!("Generation failed: {e}");}
            Update::Exported(Ok(p))=>{self.busy=false;self.status=format!("Saved {}",p.display());}
            Update::Exported(Err(e))=>{self.busy=false;self.status=format!("ZIP export failed: {e}");}
        }}
    }
    fn controls(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.add_space(16.);
        ui.horizontal(|ui|{
            ui.label(egui::RichText::new("SIMBERS").size(27.).strong().color(FG));
            ui.label(egui::RichText::new("NATIVE").size(11.).color(BLUE));
        });
        ui.label(egui::RichText::new("Rust audio engine · native OpenGL UI").small().color(MUTED));
        ui.add_space(22.);
        if button(ui,"OPEN AUDIO FILE",!self.busy).clicked(){self.browse();}
        if let Some(p)=&self.source {ui.label(egui::RichText::new(name_of(p)).small().color(MUTED));}
        ui.add_space(13.);
        ui.horizontal(|ui|{
            ui.checkbox(&mut self.options.arcs,"Arcs");
            ui.checkbox(&mut self.options.walls,"Walls");
            ui.checkbox(&mut self.options.bombs,"Bombs");
        });
        ui.checkbox(&mut self.options.bot_review,"8-bot phrase validation");
        ui.add_space(12.);
        ui.label(egui::RichText::new(format!("INTENSITY — {}%",(self.options.intensity*100.) as i32)).strong().size(12.));
        ui.add(egui::Slider::new(&mut self.options.intensity,0.1..=2.0).show_value(false));
        ui.add_space(12.);
        egui::CollapsingHeader::new("Advanced").id_salt("advanced_controls").show(ui,|ui|{
            ui.label("Song duration");
            let mut length=self.options.highlight_minutes.unwrap_or(0);
            egui::ComboBox::from_id_salt("song_duration").selected_text(if length==0{"Full song".into()}else{format!("First {length} min")})
                .show_ui(ui,|ui|{
                    ui.selectable_value(&mut length,0,"Full song");
                    for min in [5,10,12,15,30] {ui.selectable_value(&mut length,min,format!("First {min} min"));}
                });
            self.options.highlight_minutes=if length==0{None}else{Some(length)};
            ui.label("Ogg Vorbis quality");
            ui.add(egui::Slider::new(&mut self.options.vorbis_quality,0.15..=0.8));
            ui.label(egui::RichText::new("Every output mode and difficulty is included automatically.").small().color(MUTED));
        });
        ui.add_space(10.);
        if button(ui,"CREATE MAP",!self.busy && self.source.is_some()).clicked(){self.generate(ctx);}
        if button(ui,"EXPORT MAP ZIP",!self.busy && self.project.is_some()).clicked(){self.export(ctx);}
        ui.add_space(8.);
        ui.add(egui::ProgressBar::new(self.progress).desired_width(ui.available_width()).show_percentage());
        ui.label(egui::RichText::new(&self.status).size(11.).color(MUTED));
        if let Some(p)=&self.project {
            ui.add_space(14.);
            ui.separator();
            ui.label(format!("{:.1} sec · {:.1} BPM · {} charts",p.duration,p.bpm,p.charts.len()));
            ui.label(egui::RichText::new(format!("{} musical events detected",p.analysis_onsets)).small().color(MUTED));
            egui::CollapsingHeader::new("Generation timings").show(ui,|ui|{
                for (label,ms) in &p.profile {ui.label(format!("{label}: {:.1} ms",ms));}
            });
        }
    }
    fn viewer(&mut self,ui:&mut egui::Ui,ctx:&egui::Context){
        ui.add_space(12.);
        let Some(p)=&self.project else {
            ui.vertical_centered(|ui|{
                ui.add_space(70.);
                ui.label(egui::RichText::new("NO MAP LOADED").color(MUTED).size(18.));
                ui.label(egui::RichText::new("Audio analysis and viewer run locally.").color(MUTED));
            });return;
        };
        ui.horizontal(|ui|{
            ui.label(egui::RichText::new(&p.title).size(18.).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                egui::ComboBox::from_id_salt("chart_select")
                .selected_text({let c=&p.charts[self.selected_chart.min(p.charts.len()-1)];format!("{} · {}",c.name,c.characteristic)})
                .show_ui(ui,|ui|{
                    for (i,c) in p.charts.iter().enumerate(){
                        ui.selectable_value(&mut self.selected_chart,i,format!("{} · {}",c.name,c.characteristic));
                    }
                });
            });
        });
        let chart=&p.charts[self.selected_chart.min(p.charts.len()-1)];
        let available=ui.available_size();
        let height=(available.y-84.).max(150.);
        let (rect,_)=ui.allocate_exact_size(Vec2::new(available.x,height),egui::Sense::hover());
        draw_stage(ui.painter(),rect,chart,self.player.current() as f64,self.max_preview);
        ui.add_space(8.);
        ui.horizontal(|ui|{
            if ui.button(if self.player.playing{"PAUSE"}else{"PLAY"}).clicked(){self.player.toggle();}
            if ui.button("RESET").clicked(){self.player.seek(0.);}
            let mut cursor=self.player.current().min(p.duration as f32);
            let old=cursor;
            ui.add(egui::Slider::new(&mut cursor,0.0..=p.duration as f32).show_value(false));
            if (cursor-old).abs()>0.01 {self.player.seek(cursor);}
            ui.label(format!("{:02}:{:02} / {:02}:{:02}",(cursor as u32)/60,(cursor as u32)%60,
                (p.duration as u32)/60,(p.duration as u32)%60));
        });
        ui.label(egui::RichText::new(format!("{} notes  ·  {} side walls  ·  {} arcs",chart.notes.len(),chart.walls.len(),chart.arcs.len())).small().color(MUTED));
        if self.player.playing {ctx.request_repaint_after(Duration::from_millis(16));}
    }
}
fn draw_stage(painter:&egui::Painter,outer:Rect,chart:&Chart,time:f64,max_preview:usize){
    painter.rect_filled(outer,0.,Color32::from_rgb(16,16,16));
    let horizon=outer.top()+outer.height()*0.15;
    let floor=outer.bottom()-24.;
    let middle=outer.center().x;
    let lane_width=(outer.width()*0.54).min(510.);
    let depth=3.0;
    // Vanishing point and 4-column grid. No browser canvas or WebView.
    for k in 0..=4 {
        let x=middle+((k as f32)-2.)*lane_width/4.;
        painter.line_segment([Pos2::new(middle+(x-middle)*0.15,horizon),Pos2::new(x,floor)],
            Stroke::new(1.,Color32::from_gray(55)));
    }
    for row in 0..=12 {
        let z=row as f32/12.;let y=floor-(floor-horizon)*(z*z);
        painter.line_segment([Pos2::new(middle-lane_width*(0.5-0.42*z),y),
            Pos2::new(middle+lane_width*(0.5-0.42*z),y)],Stroke::new(1.,Color32::from_gray(43)));
    }
    let mut onscreen=0;
    // binary search starting index to avoid scanning entire albums every frame
    let first=chart.notes.partition_point(|n|n.time<time-0.12);
    for n in chart.notes[first..].iter().take(max_preview){
        let dt=n.time-time;
        if dt>depth {break;}
        let frac=(1.-dt/depth).clamp(0.,1.) as f32;
        let perspective=0.15+frac.powf(1.3)*0.85;
        let lane_x=n.x as f32-1.5;
        let x=middle+lane_x*(lane_width/4.)*perspective;
        let y=floor-(floor-horizon)*(1.-frac).powf(1.35)-(n.y as f32)*24.*perspective;
        let edge=(17.+27.*perspective).clamp(12.,45.);
        let color=if n.hand==0{Color32::from_rgb(235,39,77)}else{Color32::from_rgb(36,151,255)};
        painter.rect_filled(Rect::from_center_size(Pos2::new(x,y),Vec2::new(edge,edge)),3.,color);
        let arrows=["↓","↑","←","→","↙","↘","↖","↗","●"];
        painter.text(Pos2::new(x,y),Align2::CENTER_CENTER,arrows[n.dir.min(8) as usize],
            FontId::proportional(edge*0.58),Color32::WHITE);
        onscreen+=1;
    }
    painter.text(Pos2::new(outer.center().x,outer.top()+11.),Align2::CENTER_TOP,
        if onscreen==0{"WAITING FOR NOTES"}else{"SIMBERS NATIVE PREVIEW"},
        FontId::proportional(11.),MUTED);
}
impl eframe::App for App {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        self.poll();
        let mut style=(*ctx.style()).clone();
        style.visuals=egui::Visuals::dark();style.visuals.window_fill=PANEL;
        style.visuals.panel_fill=PANEL;
        style.spacing.item_spacing=Vec2::new(9.,9.);
        ctx.set_style(style);
        egui::SidePanel::left("main_controls").resizable(false).exact_width(338.).frame(egui::Frame::default().fill(PANEL).inner_margin(18.))
            .show(ctx,|ui|self.controls(ui,ctx));
        egui::CentralPanel::default().frame(egui::Frame::default().fill(PANEL).inner_margin(12.))
            .show(ctx,|ui|self.viewer(ui,ctx));
    }
}
