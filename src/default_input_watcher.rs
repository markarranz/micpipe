use std::{ffi::c_void, ptr::NonNull, sync::mpsc::Sender};

use objc2_core_audio::{
    AudioObjectAddPropertyListener, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener, kAudioHardwareNoError,
    kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
};

use crate::error::{self, Result};

pub struct DefaultInputChangeListener {
    address: AudioObjectPropertyAddress,
    sender: Box<Sender<()>>,
}

impl DefaultInputChangeListener {
    pub fn new(sender: Sender<()>) -> Result<Self> {
        let mut address = default_input_property_address();
        let sender = Box::new(sender);

        // SAFETY: `address` points to initialized memory for the duration of the call.
        // `sender` stays boxed inside the returned listener until Drop removes the
        // property listener, so Core Audio never observes a dangling client pointer.
        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut address),
                Some(default_input_changed),
                client_data(&sender),
            )
        };

        if status != kAudioHardwareNoError {
            return Err(error::message(format!(
                "failed to watch default input device changes: Core Audio status {status}"
            )));
        }

        Ok(Self { address, sender })
    }
}

impl Drop for DefaultInputChangeListener {
    fn drop(&mut self) {
        // SAFETY: `self.address` and `self.sender` are the same values used to register
        // the callback. They remain valid for the duration of the removal call.
        let _ = unsafe {
            AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut self.address),
                Some(default_input_changed),
                client_data(&self.sender),
            )
        };
    }
}

fn client_data(sender: &Sender<()>) -> *mut c_void {
    std::ptr::from_ref(sender).cast_mut().cast::<c_void>()
}

fn default_input_property_address() -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

unsafe extern "C-unwind" fn default_input_changed(
    _object_id: AudioObjectID,
    number_addresses: u32,
    addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    if client_data.is_null() {
        return kAudioHardwareNoError;
    }

    // SAFETY: Core Audio passes `number_addresses` initialized addresses at `addresses`.
    let addresses =
        unsafe { std::slice::from_raw_parts(addresses.as_ptr(), number_addresses as usize) };
    if addresses
        .iter()
        .any(|address| address.mSelector == kAudioHardwarePropertyDefaultInputDevice)
    {
        // SAFETY: `client_data` is created from the boxed `Sender<()>` owned by
        // `DefaultInputChangeListener` and remains valid while this listener is registered.
        let sender = unsafe { &*(client_data.cast::<Sender<()>>()) };
        let _ = sender.send(());
    }

    kAudioHardwareNoError
}
