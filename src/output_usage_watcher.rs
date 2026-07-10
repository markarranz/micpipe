//! Polls Core Audio to detect active input clients for the selected output.

use std::{
    ffi::c_void,
    mem::{MaybeUninit, size_of},
    ptr::{NonNull, null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use cpal::traits::DeviceTrait;
use objc2_core_audio::{
    AudioDeviceID, AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, AudioObjectPropertyScope, kAudioHardwareNoError,
    kAudioHardwarePropertyDevices, kAudioHardwarePropertyProcessObjectList,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectSystemObject, kAudioProcessPropertyDevices,
    kAudioProcessPropertyIsRunningInput, kAudioProcessPropertyPID,
};
use objc2_core_foundation::{CFRetained, CFString};

use crate::router::AudioControl;

const OUTPUT_USAGE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Watches whether another process is actively using the selected device as an input.
///
/// The watcher deliberately runs without a micpipe output stream. Starting an output stream on
/// the selected device would make the device look active even when no application was reading its
/// input side.
pub(crate) struct OutputUsageWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OutputUsageWatcher {
    pub(crate) fn start(
        output_device: &cpal::Device,
        sender: Sender<AudioControl>,
    ) -> Result<Self> {
        let output_uid = output_device
            .id()
            .context("failed to identify output device")?
            .id()
            .to_owned();
        let output_device_id = find_device_id(&output_uid)?;
        let initially_in_use = output_is_used_as_input(output_device_id)?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("micpipe-output-usage".to_string())
            .spawn(move || {
                let mut last_in_use = initially_in_use;
                if sender
                    .send(AudioControl::OutputUsageChanged(initially_in_use))
                    .is_err()
                {
                    return;
                }

                while !thread_stop.load(Ordering::Relaxed) {
                    thread::sleep(OUTPUT_USAGE_POLL_INTERVAL);
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }

                    let in_use = match output_is_used_as_input(output_device_id) {
                        Ok(in_use) => in_use,
                        Err(err) => {
                            crate::log_err!("failed to inspect output input usage: {}", err);
                            if last_in_use {
                                last_in_use = false;
                                if sender
                                    .send(AudioControl::OutputUsageChanged(false))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            continue;
                        }
                    };

                    if in_use == last_in_use {
                        continue;
                    }

                    last_in_use = in_use;
                    if sender
                        .send(AudioControl::OutputUsageChanged(in_use))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|err| anyhow!("failed to start output usage watcher: {err}"))?;

        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for OutputUsageWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn find_device_id(uid: &str) -> Result<AudioDeviceID> {
    // The selected CPAL device ID is its CoreAudio UID, so enumerate the system devices through
    // the same HAL property API and compare UIDs exactly.
    let device_ids = property_data_vec::<AudioDeviceID>(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain,
        "CoreAudio device list",
    )?;

    for device_id in device_ids {
        if let Ok(device_uid) = device_uid(device_id)
            && device_uid == uid
        {
            return Ok(device_id);
        }
    }

    Err(anyhow!(
        "could not resolve CoreAudio output device UID '{uid}'"
    ))
}

fn output_is_used_as_input(output_device_id: AudioDeviceID) -> Result<bool> {
    let process_ids = property_data_vec::<AudioObjectID>(
        kAudioObjectSystemObject as AudioObjectID,
        kAudioHardwarePropertyProcessObjectList,
        kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain,
        "CoreAudio process object list",
    )?;
    let own_pid = std::process::id() as libc::pid_t;

    for process_id in process_ids {
        let process_pid = match property_data::<libc::pid_t>(
            process_id,
            kAudioProcessPropertyPID,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain,
            "CoreAudio process PID",
        ) {
            Ok(pid) => pid,
            Err(_) => continue,
        };

        if process_pid == own_pid {
            continue;
        }

        let is_running_input = match property_data::<u32>(
            process_id,
            kAudioProcessPropertyIsRunningInput,
            kAudioObjectPropertyScopeGlobal,
            kAudioObjectPropertyElementMain,
            "CoreAudio process input state",
        ) {
            Ok(running) => running != 0,
            Err(_) => continue,
        };

        if !is_running_input {
            continue;
        }

        let input_devices = match property_data_vec::<AudioDeviceID>(
            process_id,
            kAudioProcessPropertyDevices,
            kAudioObjectPropertyScopeInput,
            kAudioObjectPropertyElementMain,
            "CoreAudio process input devices",
        ) {
            Ok(devices) => devices,
            Err(_) => continue,
        };

        if process_uses_input_device(&input_devices, output_device_id) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn process_uses_input_device(input_devices: &[AudioDeviceID], target: AudioDeviceID) -> bool {
    input_devices.contains(&target)
}

fn device_uid(device_id: AudioDeviceID) -> Result<String> {
    let address = AudioObjectPropertyAddress {
        mSelector: objc2_core_audio::kAudioDevicePropertyDeviceUID,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut uid: *mut CFString = null_mut();
    let mut data_size = size_of::<*mut CFString>() as u32;

    // SAFETY: All pointers refer to initialized storage that remains valid for the duration of
    // the synchronous CoreAudio call. CoreAudio writes one retained CFString pointer to `uid`.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut uid).cast(),
        )
    };
    check_status(status, "failed to read CoreAudio device UID")?;

    let uid = NonNull::new(uid).ok_or_else(|| anyhow!("CoreAudio returned a null device UID"))?;

    // SAFETY: The successful property query returns a CFString retained for the caller.
    Ok(unsafe { CFRetained::from_raw(uid).to_string() })
}

fn property_data<T: Copy>(
    object_id: AudioObjectID,
    selector: objc2_core_audio::AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: u32,
    description: &str,
) -> Result<T> {
    let address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: element,
    };
    let mut value = MaybeUninit::<T>::uninit();
    let mut data_size = size_of::<T>() as u32;

    // SAFETY: The address and output buffer are valid for the duration of the synchronous call.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
            NonNull::from(&mut value).cast(),
        )
    };
    check_status(status, description)?;
    if (data_size as usize) < size_of::<T>() {
        return Err(anyhow!(
            "{description}: CoreAudio returned {data_size} bytes, expected {}",
            size_of::<T>()
        ));
    }

    // SAFETY: A successful query with a full-sized result initialized every byte of `value`.
    Ok(unsafe { value.assume_init() })
}

fn property_data_vec<T: Copy>(
    object_id: AudioObjectID,
    selector: objc2_core_audio::AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: u32,
    description: &str,
) -> Result<Vec<T>> {
    let address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: element,
    };
    let mut data_size = 0u32;

    // SAFETY: The address and output size pointer are valid for the duration of the synchronous
    // size query; this property does not require qualifier data.
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut data_size),
        )
    };
    check_status(status, description)?;

    let byte_count = data_size as usize;
    if byte_count == 0 {
        return Ok(Vec::new());
    }
    if !byte_count.is_multiple_of(size_of::<T>()) {
        return Err(anyhow!(
            "{description}: CoreAudio returned {data_size} bytes, which is not a multiple of {}",
            size_of::<T>()
        ));
    }

    let capacity = byte_count / size_of::<T>();
    let mut values = Vec::<T>::with_capacity(capacity);
    let mut actual_size = data_size;

    // SAFETY: CoreAudio writes at most `capacity * size_of::<T>()` bytes into the allocated
    // vector. The vector length is set only after the call reports how many bytes were written.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&address),
            0,
            null(),
            NonNull::from(&mut actual_size),
            NonNull::new(values.as_mut_ptr().cast::<c_void>()).expect("non-empty allocation"),
        )
    };
    check_status(status, description)?;

    let actual_byte_count = actual_size as usize;
    if actual_byte_count > byte_count || !actual_byte_count.is_multiple_of(size_of::<T>()) {
        return Err(anyhow!(
            "{description}: CoreAudio returned an invalid data size of {actual_size}"
        ));
    }

    // SAFETY: The successful query initialized exactly this many T values in `values`.
    unsafe {
        values.set_len(actual_byte_count / size_of::<T>());
    }
    Ok(values)
}

fn check_status(status: i32, description: &str) -> Result<()> {
    if status == kAudioHardwareNoError {
        Ok(())
    } else {
        Err(anyhow!("{description}: CoreAudio status {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::process_uses_input_device;

    #[test]
    fn matches_only_the_selected_input_device() {
        assert!(process_uses_input_device(&[10, 20], 20));
        assert!(!process_uses_input_device(&[10, 20], 30));
    }
}
