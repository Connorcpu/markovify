use winapi;
use ole32;
use std::iter::Extend;
use std::mem;
use std::ptr;
use std::thread;
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use self::SpeechMessage::*;

pub struct Speechifier {
    mailbox: Option<SyncSender<SpeechMessage>>,
}

enum SpeechMessage {
    Word(String),
    Stop,
}

impl Speechifier {
    pub fn new() -> Speechifier {
        Speechifier { mailbox: None }
    }

    pub fn start(&mut self) {
        let (tx, rx) = sync_channel(0);
        self.mailbox = Some(tx);

        thread::spawn(move || {
            unsafe {
                speechify(rx);
            }
        });
    }

    pub fn stop(&mut self) {
        if let Some(ref mailbox) = self.mailbox {
            mailbox.send(Stop).unwrap();
        }

        self.mailbox = None;
    }

    pub fn queue(&self, word: String) {
        if let Some(ref mailbox) = self.mailbox {
            mailbox.send(Word(word)).unwrap();
        }
    }
}

#[inline]
fn failed(hr: winapi::HRESULT) -> bool {
    hr < 0
}

#[inline]
fn succeeded(hr: winapi::HRESULT) -> bool {
    !failed(hr)
}

pub trait ToWide {
    fn to_wide(&self) -> Vec<u16>;
    fn to_wide_null(&self) -> Vec<u16>;
}

impl<T> ToWide for T where T: AsRef<OsStr> {
    fn to_wide(&self) -> Vec<u16> {
        self.as_ref().encode_wide().collect()
    }
    fn to_wide_null(&self) -> Vec<u16> {
        self.as_ref().encode_wide().chain(Some(0)).collect()
    }
}

unsafe fn speechify(rx: Receiver<SpeechMessage>) {
    let mut hr;
    let mut voice: *mut winapi::ISpVoice = ptr::null_mut();

    hr = ole32::CoInitialize(ptr::null_mut());
    if failed(hr) {
        return;
    }

    let sp_voice = "SAPI.SpVoice".to_wide_null();
    let mut clsid_spvoice: winapi::CLSID = mem::uninitialized();

    hr = ole32::CLSIDFromProgID(&sp_voice[0], &mut clsid_spvoice);
    if failed(hr) {
        return;
    }

    hr = ole32::CoCreateInstance(
        &clsid_spvoice,
        ptr::null_mut(),
        winapi::CLSCTX_ALL,
        &winapi::UuidOfISpVoice,
        &mut voice as *mut *mut winapi::ISpVoice as *mut *mut winapi::c_void
    );

    if succeeded(hr) {
        (*voice).SetRate(2);
        speech_loop(rx, &mut *voice);
        (*voice).Release();
    }

    ole32::CoUninitialize();
}

unsafe fn speech_loop(rx: Receiver<SpeechMessage>, voice: &mut winapi::ISpVoice) {
    let mut buffer: Vec<u16> = Vec::new();
    loop {
        if let Ok(Word(word)) = rx.recv() {
            buffer.extend(word.utf16_units());
            buffer.push(0);
        } else {
            return;
        }

        voice.Speak(buffer.as_ptr(), 0, ptr::null_mut());
        voice.WaitUntilDone(winapi::INFINITE);

        buffer.clear();
    }
}
