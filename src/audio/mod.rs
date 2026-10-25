//! Sound: an OPL2 (Sound Blaster FM) emulator playing CMF music, VOC samples,
//! and a mixer producing PCM at the output device's rate.

pub mod cmf;
pub mod opl2;
pub mod voc;

use cmf::CmfPlayer;
use opl2::OPL_RATE;
use voc::Voice;

pub struct Mixer {
    pub out_rate: f64,
    pub music: CmfPlayer,
    pub music_volume: f32,
    pub sfx_volume: f32,
    voice: Option<(Voice, f64)>,
    opl_buf: Vec<f32>,
    opl_pos: f64,
    opl_len: usize,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new(44100.0)
    }
}

impl Mixer {
    pub fn new(out_rate: f64) -> Self {
        Mixer {
            out_rate,
            music: CmfPlayer::new(),
            music_volume: 1.0,
            sfx_volume: 1.0,
            voice: None,
            opl_buf: vec![0.0; 4096],
            opl_pos: 0.0,
            opl_len: 0,
        }
    }

    pub fn play_music(&mut self, bytes: &[u8]) -> Result<(), crate::Error> {
        self.music.looping = true;
        self.music.load(bytes)
    }

    pub fn stop_music(&mut self) {
        self.music.stop();
    }

    pub fn play_voice(&mut self, v: Voice) {
        self.voice = Some((v, 0.0));
    }

    /// Mix into `out` (mono samples at `out_rate`).
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let step = OPL_RATE / self.out_rate;
        if self.music.has_song() && self.music.playing {
            for o in out.iter_mut() {
                while self.opl_pos + 1.0 >= self.opl_len as f64 {
                    let keep = if self.opl_len > 0 { self.opl_buf[self.opl_len - 1] } else { 0.0 };
                    if self.opl_len > 0 {
                        self.opl_pos -= (self.opl_len - 1) as f64;
                    }
                    self.opl_buf[0] = keep;
                    let len = self.opl_buf.len();
                    self.music.render(&mut self.opl_buf[1..len]);
                    self.opl_len = len;
                }
                let k = self.opl_pos.floor() as usize;
                let f = (self.opl_pos - k as f64) as f32;
                *o = (self.opl_buf[k] * (1.0 - f) + self.opl_buf[k + 1] * f) * self.music_volume;
                self.opl_pos += step;
            }
        }
        if let Some((v, pos)) = &mut self.voice {
            let vs = v.rate / self.out_rate;
            for o in out.iter_mut() {
                if *pos as usize >= v.samples.len() {
                    break;
                }
                *o += v.samples[*pos as usize] * self.sfx_volume;
                *pos += vs;
            }
            if *pos as usize >= v.samples.len() {
                self.voice = None;
            }
        }
    }
}
