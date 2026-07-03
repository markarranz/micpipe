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
    sender: *mut Sender<()>,
}

impl DefaultInputChangeListener {
    pub fn new(sender: Sender<()>) -> Result<Self> {
        let mut address = default_input_property_address();
        let sender = Box::into_raw(Box::new(sender));
        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut address),
                Some(default_input_changed),
                sender.cast::<c_void>(),
            )
        };

        if status != kAudioHardwareNoError {
            unsafe {
                drop(Box::from_raw(sender));
            }
            return Err(error::message(format!(
                "failed to watch default input device changes: Core Audio status {status}"
            )));
        }

        Ok(Self { address, sender })
    }
}

impl Drop for DefaultInputChangeListener {
    fn drop(&mut self) {
        let _ = unsafe {
            AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut self.address),
                Some(default_input_changed),
                self.sender.cast::<c_void>(),
            )
        };
        unsafe {
            drop(Box::from_raw(self.sender));
        }
    }
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

    let addresses =
        unsafe { std::slice::from_raw_parts(addresses.as_ptr(), number_addresses as usize) };
    if addresses
        .iter()
        .any(|address| address.mSelector == kAudioHardwarePropertyDefaultInputDevice)
    {
        let sender = unsafe { &*(client_data.cast::<Sender<()>>()) };
        let _ = sender.send(());
    }

    kAudioHardwareNoError
}
