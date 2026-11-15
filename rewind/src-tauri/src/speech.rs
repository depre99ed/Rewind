//! Reconnaissance de « Rewind clip » directement dans Rewind (2026-11-15).
//!
//! Même moteur que l'ancien script PowerShell : la reconnaissance vocale de Windows (SAPI 5, le moteur
//! que System.Speech utilisait), hors ligne, avec une grammaire d'une seule phrase. Le son du micro
//! choisi dans Rewind (PCM 16 kHz mono 16 bits) arrive par un canal au lieu de l'entrée standard.
//!
//! Pourquoi : lancer un PowerShell caché avec `-ExecutionPolicy Bypass` sur un script écrit dans AppData,
//! qui compile du C# à la volée (`Add-Type`), est exactement ce que font les logiciels malveillants.
//! Windows Defender a mis Rewind en quarantaine chez un utilisateur.
//!
//! Le fil produit les mêmes lignes que le script : `READY <langue>`, `CLIP <confiance> <durée>s <texte>`,
//! `HEARD <confiance> <durée>s [rewind] <texte>`, `ERR <message>` (voice.rs les lit comme avant).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::Foundation::{E_NOTIMPL, S_OK, STG_E_ACCESSDENIED};
use windows::Win32::Media::Audio::WAVEFORMATEX;
use windows::Win32::Media::Speech::*;
use windows::Win32::System::Com::*;
use windows::core::{GUID, HRESULT, Interface, PCWSTR, Result, implement, w};

/// Format « WAVEFORMATEX » de SAPI (SPDFID_WaveFormatEx, absent des déclarations Rust).
const SPDFID_WAVEFORMATEX: GUID = GUID::from_u128(0xc31adbae_527f_4ff5_a230_f62bb61ff70c);

/// Réglages (mêmes noms et valeurs par défaut que le script : -MinConfidence 0.70, -MaxSeconds 1.15).
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub min_confidence: f32,
    pub max_seconds: f64,
    /// Seuil de rejet du moteur (CFGConfidenceRejectionThreshold), -1 = celui de Windows
    pub rejection: i32,
    /// Écrit aussi les phrases entendues sous le seuil (lignes HEARD)
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { min_confidence: 0.70, max_seconds: 1.15, rejection: -1, verbose: true }
    }
}

impl Options {
    /// Réglage pendant les tests : `-MinConfidence 0.6 -MaxSeconds 1.2 -Rejection 40`.
    pub fn with_args(mut self, args: &str) -> Self {
        let words: Vec<&str> = args.split_whitespace().collect();
        for pair in words.windows(2) {
            match pair[0] {
                "-MinConfidence" => self.min_confidence = pair[1].replace(',', ".").parse().unwrap_or(self.min_confidence),
                "-MaxSeconds" => self.max_seconds = pair[1].replace(',', ".").parse().unwrap_or(self.max_seconds),
                "-Rejection" => self.rejection = pair[1].parse().unwrap_or(self.rejection),
                _ => {}
            }
        }
        self
    }
}

/// Moteur en marche : on lui envoie le son, on lit ses lignes, on l'arrête.
pub struct Engine {
    pub audio: Sender<Vec<u8>>,
    pub lines: Receiver<String>,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    /// Le moteur s'est arrêté tout seul (erreur, fin du son).
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub fn start(options: Options) -> std::io::Result<Engine> {
    let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (s, d) = (Arc::clone(&stop), Arc::clone(&done));
    let thread = std::thread::Builder::new().name("voix-moteur".into()).spawn(move || {
        if let Err(e) = recognize(options, audio_rx, &line_tx, &s) {
            let _ = line_tx.send(format!("ERR {}", e.message()));
        }
        d.store(true, Ordering::SeqCst);
    })?;
    Ok(Engine { audio: audio_tx, lines: line_rx, stop, done, thread: Some(thread) })
}

// ---------------------------------------------------------------------------
// Flux de son lu par SAPI : attend d'avoir tout ce qui est demandé (une lecture courte = fin du flux),
// se dit « déplaçable » sans l'être (SAPI le demande), comme l'enveloppe `RewindPipe` du script.
// ---------------------------------------------------------------------------

#[implement(IStream)]
struct PcmPipe {
    state: Mutex<PipeState>,
    stop: Arc<AtomicBool>,
}

struct PipeState {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
    pos: u64,
    ended: bool,
}

impl ISequentialStream_Impl for PcmPipe_Impl {
    fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        let Ok(mut st) = self.state.lock() else { return S_OK };
        let want = cb as usize;
        while st.pending.len() < want && !st.ended {
            if self.stop.load(Ordering::SeqCst) {
                st.ended = true;
                break;
            }
            match st.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(chunk) => st.pending.extend_from_slice(&chunk),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => st.ended = true,
            }
        }
        let n = want.min(st.pending.len());
        if n > 0 && !pv.is_null() {
            unsafe { std::ptr::copy_nonoverlapping(st.pending.as_ptr(), pv as *mut u8, n) };
        }
        st.pending.drain(..n);
        st.pos += n as u64;
        if !pcbread.is_null() {
            unsafe { *pcbread = n as u32 };
        }
        S_OK
    }

    fn Write(&self, _pv: *const core::ffi::c_void, _cb: u32, _pcbwritten: *mut u32) -> HRESULT {
        STG_E_ACCESSDENIED
    }
}

impl IStream_Impl for PcmPipe_Impl {
    fn Seek(&self, _dlibmove: i64, _dworigin: STREAM_SEEK, plibnewposition: *mut u64) -> Result<()> {
        let pos = self.state.lock().map(|s| s.pos).unwrap_or(0);
        if !plibnewposition.is_null() {
            unsafe { *plibnewposition = pos };
        }
        Ok(())
    }
    fn SetSize(&self, _libnewsize: u64) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn CopyTo(&self, _pstm: windows::core::Ref<IStream>, _cb: u64, _pcbread: *mut u64, _pcbwritten: *mut u64) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Commit(&self, _grfcommitflags: &STGC) -> Result<()> {
        Ok(())
    }
    fn Revert(&self) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn LockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: &LOCKTYPE) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn UnlockRegion(&self, _liboffset: u64, _cb: u64, _dwlocktype: u32) -> Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> Result<()> {
        if !pstatstg.is_null() {
            unsafe {
                *pstatstg = STATSTG::default();
                (*pstatstg).r#type = STGTY_STREAM.0 as u32;
                (*pstatstg).cbSize = i64::MAX as u64; // longueur inconnue (micro)
            }
        }
        Ok(())
    }
    fn Clone(&self) -> Result<IStream> {
        Err(E_NOTIMPL.into())
    }
}

// ---------------------------------------------------------------------------

/// Langue d'un moteur (« 409;9 » → 0x409).
fn token_lang(token: &ISpObjectToken) -> Option<u16> {
    unsafe {
        let attrs = token.OpenKey(w!("Attributes")).ok()?;
        let value = attrs.GetStringValue(w!("Language")).ok()?;
        let text = value.to_string().ok();
        CoTaskMemFree(Some(value.0 as _));
        let first = text?.split(';').next()?.trim().to_string();
        u16::from_str_radix(&first, 16).ok()
    }
}

fn lang_name(lang: u16) -> String {
    let mut buf = [0u16; 85];
    let n = unsafe { windows_sys::Win32::Globalization::LCIDToLocaleName(lang as u32, buf.as_mut_ptr(), buf.len() as i32, 0) };
    if n > 1 { String::from_utf16_lossy(&buf[..(n - 1) as usize]) } else { format!("{lang:x}") }
}

/// Moteur anglais de préférence (« Rewind clip » est de l'anglais), sinon le premier installé.
fn pick_recognizer() -> Result<Option<(ISpObjectToken, u16)>> {
    unsafe {
        let cat: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        cat.SetId(SPCAT_RECOGNIZERS, false)?;
        let list = cat.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
        let mut first = None;
        loop {
            let mut tok: [Option<ISpObjectToken>; 1] = [None];
            let mut got = 0u32;
            if list.Next(1, tok.as_mut_ptr(), Some(&mut got)).is_err() || got == 0 {
                break;
            }
            let Some(token) = tok[0].take() else { break };
            let lang = token_lang(&token).unwrap_or(0x409);
            if lang & 0x3ff == 0x09 {
                return Ok(Some((token, lang)));
            }
            if first.is_none() {
                first = Some((token, lang));
            }
        }
        Ok(first)
    }
}

/// Libère ce que porte un événement SAPI (objet COM ou mémoire), comme `SpClearEvent`.
unsafe fn clear_event(ev: &SPEVENT) {
    let kind = (ev._bitfield >> 16) & 0xffff;
    let lparam = ev.lParam.0;
    if lparam == 0 {
        return;
    }
    unsafe {
        if kind == SPET_LPARAM_IS_OBJECT.0 || kind == SPET_LPARAM_IS_TOKEN.0 {
            drop(windows::core::IUnknown::from_raw(lparam as *mut core::ffi::c_void));
        } else if kind == SPET_LPARAM_IS_POINTER.0 || kind == SPET_LPARAM_IS_STRING.0 {
            CoTaskMemFree(Some(lparam as *const core::ffi::c_void));
        }
    }
}

fn spfei(e: SPEVENTENUM) -> u64 {
    (1u64 << e.0) | (1u64 << SPEI_RESERVED1.0) | (1u64 << SPEI_RESERVED2.0)
}

fn recognize(opt: Options, audio: Receiver<Vec<u8>>, out: &Sender<String>, stop: &Arc<AtomicBool>) -> Result<()> {
    unsafe {
        // Appartement « multi-fils » propre à ce fil
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| -> Result<()> {
            let Some((token, lang)) = pick_recognizer()? else {
                let _ = out.send("ERR aucun moteur de reconnaissance vocale installe".into());
                return Ok(());
            };
            let reco: ISpRecognizer = CoCreateInstance(&SpInprocRecognizer, None, CLSCTX_ALL)?;
            reco.SetRecognizer(&token)?;

            // Entrée : le son envoyé par Rewind (16 kHz, 16 bits, mono)
            let fmt = WAVEFORMATEX { wFormatTag: 1, nChannels: 1, nSamplesPerSec: 16_000, nAvgBytesPerSec: 32_000, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
            let pipe: IStream = PcmPipe {
                state: Mutex::new(PipeState { rx: audio, pending: Vec::new(), pos: 0, ended: false }),
                stop: Arc::clone(stop),
            }
            .into();
            let stream: ISpStream = CoCreateInstance(&SpStream, None, CLSCTX_ALL)?;
            stream.SetBaseStream(&pipe, &SPDFID_WAVEFORMATEX, &fmt)?;
            reco.SetInput(&stream, true)?;
            if opt.rejection >= 0 {
                let _ = reco.SetPropertyNum(w!("CFGConfidenceRejectionThreshold"), opt.rejection);
            }

            let ctx = reco.CreateRecoContext()?;
            ctx.SetNotifyWin32Event()?;
            let interest = spfei(SPEI_RECOGNITION) | spfei(SPEI_END_SR_STREAM);
            ctx.SetInterest(interest, interest)?;

            // Une seule phrase : les variantes faisaient accepter « remind me to clip it »
            let grammar = ctx.CreateGrammar(1)?;
            grammar.ResetGrammar(lang)?;
            let mut state = SPSTATEHANDLE::default();
            grammar.GetRule(w!("rewind"), 0, (SPRAF_TopLevel.0 | SPRAF_Active.0) as u32, true, &mut state)?;
            grammar.AddWordTransition(state, SPSTATEHANDLE::default(), w!("rewind clip"), w!(" "), SPWT_LEXICAL, 1.0, std::ptr::null())?;
            grammar.Commit(0)?;
            grammar.SetRuleState(PCWSTR::null(), std::ptr::null_mut(), SPRS_ACTIVE)?;
            reco.SetRecoState(SPRST_ACTIVE)?;
            let _ = out.send(format!("READY {}", lang_name(lang)));

            'outer: while !stop.load(Ordering::SeqCst) {
                let _ = ctx.WaitForNotifyEvent(300);
                loop {
                    let mut ev = SPEVENT::default();
                    let mut fetched = 0u32;
                    if ctx.GetEvents(1, &mut ev, &mut fetched).is_err() || fetched == 0 {
                        break;
                    }
                    let id = ev._bitfield & 0xffff;
                    if id == SPEI_END_SR_STREAM.0 {
                        clear_event(&ev);
                        break 'outer; // fin du son (Rewind arrête l'écoute)
                    }
                    if id == SPEI_RECOGNITION.0 && ev.lParam.0 != 0 {
                        let res = ISpRecoResult::from_raw(ev.lParam.0 as *mut core::ffi::c_void);
                        report(&res, &opt, out);
                        drop(res); // libère l'objet porté par l'événement
                        continue;
                    }
                    clear_event(&ev);
                }
            }
            let _ = reco.SetRecoState(SPRST_INACTIVE);
            drop(grammar);
            drop(ctx);
            drop(reco);
            Ok(())
        })();
        CoUninitialize();
        result
    }
}

/// Une phrase reconnue : CLIP si assez sûre et assez courte, sinon HEARD (pour le réglage du seuil).
unsafe fn report(res: &ISpRecoResult, opt: &Options, out: &Sender<String>) {
    unsafe {
        let Ok(phrase) = res.GetPhrase() else { return };
        if phrase.is_null() {
            return;
        }
        let confidence = (*phrase).Base.Rule.SREngineConfidence;
        CoTaskMemFree(Some(phrase as *const core::ffi::c_void));
        let mut times = SPRECORESULTTIMES::default();
        let seconds = if res.GetResultTimes(&mut times).is_ok() { times.ullLength as f64 / 10_000_000.0 } else { 0.0 };
        if confidence >= opt.min_confidence && (opt.max_seconds <= 0.0 || seconds <= opt.max_seconds) {
            let _ = out.send(format!("CLIP {confidence:.2} {seconds:.2}s rewind clip"));
        } else if opt.verbose {
            let _ = out.send(format!("HEARD {confidence:.2} {seconds:.2}s [rewind] rewind clip"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reglages_de_test_lus_comme_le_script() {
        let o = Options::default().with_args("-MinConfidence 0,6 -MaxSeconds 1.3 -Rejection 40");
        assert!((o.min_confidence - 0.6).abs() < 1e-6);
        assert!((o.max_seconds - 1.3).abs() < 1e-9);
        assert_eq!(o.rejection, 40);
        let d = Options::default();
        assert!((d.min_confidence - 0.70).abs() < 1e-6 && (d.max_seconds - 1.15).abs() < 1e-9);
    }
}
