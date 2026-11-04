// FLITZIS-LOOPER downstream: exact IAudioClient3 shared-period configuration.
// See ../../../looper-provenance.json for the original source and patch hashes.
use crate::{
    BackendSpecificError, BufferSize, Data, DefaultStreamConfigError, DeviceDescription,
    DeviceDescriptionBuilder, DeviceDirection, DeviceId, DeviceIdError, DeviceNameError,
    DeviceType, DevicesError, FrameCount, InputCallbackInfo, InterfaceType, OutputCallbackInfo,
    SampleFormat, SampleRate, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange, SupportedStreamConfigsError, COMMON_SAMPLE_RATES,
};

impl From<Audio::EDataFlow> for DeviceDirection {
    fn from(data_flow: Audio::EDataFlow) -> Self {
        if data_flow == Audio::eCapture {
            DeviceDirection::Input
        } else if data_flow == Audio::eRender {
            DeviceDirection::Output
        } else {
            DeviceDirection::Unknown
        }
    }
}
use std::ffi::OsString;
use std::fmt;
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::ptr;
use std::slice;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::com;
use super::{windows_err_to_cpal_err, windows_err_to_cpal_err_message};
use windows::core::Interface;
use windows::core::GUID;
use windows::Win32::Devices::Properties;
use windows::Win32::Foundation;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::IAudioRenderClient;
use windows::Win32::Media::{Audio, KernelStreaming, Multimedia};
use windows::Win32::System::Com;
use windows::Win32::System::Com::{StructuredStorage, STGM_READ};
use windows::Win32::System::Threading;
use windows::Win32::System::Variant::{VT_LPWSTR, VT_UI4};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use super::stream::{AudioClientFlow, Stream, StreamInner};
use crate::{traits::DeviceTrait, BuildStreamError, StreamError};

pub use crate::iter::{SupportedInputConfigs, SupportedOutputConfigs};

/// Periods supported by IAudioClient3 for one exact shared-mode output format.
/// These are engine periods, not callback lengths or end-to-end latency.
#[derive(Clone, Copy, Debug)]
pub struct SharedOutputEnginePeriods {
    pub default_frames: u32,
    pub fundamental_frames: u32,
    pub min_frames: u32,
    pub max_frames: u32,
}

impl SharedOutputEnginePeriods {
    pub fn supports(&self, frames: u32) -> bool {
        self.fundamental_frames != 0
            && frames >= self.min_frames
            && frames <= self.max_frames
            && frames % self.fundamental_frames == 0
    }

    fn select(&self, requested: Option<u32>) -> Result<u32, BuildStreamError> {
        let frames = requested.unwrap_or(self.default_frames);
        if self.min_frames == 0 || !self.supports(self.default_frames) || !self.supports(frames) {
            return Err(BackendSpecificError {
                description: format!("Unsupported shared engine period {frames}: {self:?}"),
            }
            .into());
        }
        Ok(frames)
    }
}

/// Values returned by the initialized client, before its render thread starts.
#[derive(Clone, Debug)]
pub struct SharedOutputStreamInfo {
    pub periods: SharedOutputEnginePeriods,
    pub initialized_period_frames: u32,
    pub buffer_capacity_frames: u32,
    pub reported_latency_100ns: i64,
    /// An instantaneous shared-engine observation, not an ongoing guarantee.
    pub current_period_frames: Option<u32>,
    pub current_sample_rate_hz: Option<u32>,
    pub current_channels: Option<u16>,
    pub current_sample_format: Option<SampleFormat>,
    pub current_period_error: Option<String>,
}

enum OutputInitialization {
    Legacy,
    SharedPeriod {
        frames: Option<u32>,
        match_format: bool,
    },
}

// Own an event until all fallible stream construction has completed.
struct PendingEvent(Option<Foundation::HANDLE>);

impl Drop for PendingEvent {
    fn drop(&mut self) {
        if let Some(event) = self.0.take() {
            unsafe {
                let _ = Foundation::CloseHandle(event);
            }
        }
    }
}

fn shared_period_error(operation: &str, error: windows::core::Error) -> BuildStreamError {
    BackendSpecificError {
        description: format!("{operation}: {error} (HRESULT {:#010x})", error.code().0),
    }
    .into()
}

fn exact_shared_format_result(result: windows::core::HRESULT) -> Result<(), BuildStreamError> {
    match result {
        Foundation::S_OK => Ok(()),
        Foundation::S_FALSE | Audio::AUDCLNT_E_UNSUPPORTED_FORMAT => {
            Err(BuildStreamError::StreamConfigNotSupported)
        }
        failure => Err(shared_period_error(
            "IsFormatSupported",
            windows::core::Error::from_hresult(failure),
        )),
    }
}

unsafe fn require_exact_shared_format(
    client: &Audio::IAudioClient,
    format: &Audio::WAVEFORMATEX,
) -> Result<(), BuildStreamError> {
    let mut closest = ptr::null_mut();
    let result =
        client.IsFormatSupported(Audio::AUDCLNT_SHAREMODE_SHARED, format, Some(&mut closest));
    let _closest_owner = WaveFormatExPtr(closest);
    exact_shared_format_result(result)
}

unsafe fn configure_shared_client(
    client: &Audio::IAudioClient3,
    match_format: bool,
) -> Result<(), BuildStreamError> {
    let properties = Audio::AudioClientProperties {
        cbSize: mem::size_of::<Audio::AudioClientProperties>() as u32,
        bIsOffload: false.into(),
        eCategory: Audio::AudioCategory_Media,
        Options: if match_format {
            Audio::AUDCLNT_STREAMOPTIONS_MATCH_FORMAT
        } else {
            Audio::AUDCLNT_STREAMOPTIONS_NONE
        },
    };
    client
        .SetClientProperties(&properties)
        .map_err(|error| shared_period_error("SetClientProperties", error))
}

unsafe fn query_shared_periods(
    client: &Audio::IAudioClient3,
    format: &Audio::WAVEFORMATEX,
) -> Result<SharedOutputEnginePeriods, BuildStreamError> {
    let mut periods = SharedOutputEnginePeriods {
        default_frames: 0,
        fundamental_frames: 0,
        min_frames: 0,
        max_frames: 0,
    };
    client
        .GetSharedModeEnginePeriod(
            format,
            &mut periods.default_frames,
            &mut periods.fundamental_frames,
            &mut periods.min_frames,
            &mut periods.max_frames,
        )
        .map_err(|error| shared_period_error("GetSharedModeEnginePeriod", error))?;
    if periods.min_frames == 0 || !periods.supports(periods.default_frames) {
        return Err(BackendSpecificError {
            description: "GetSharedModeEnginePeriod returned incoherent periods".into(),
        }
        .into());
    }
    Ok(periods)
}

// PKEY_AudioEndpoint properties not yet in windows-rs

/// PKEY_AudioEndpoint_FormFactor (PID 0) - VT_UI4 containing EndpointFormFactor enum
const PKEY_AUDIOENDPOINT_FORMFACTOR: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
    pid: 0,
};

/// PKEY_AudioEndpoint_JackSubType (PID 8) - VT_LPWSTR containing KS node type GUID
const PKEY_AUDIOENDPOINT_JACKSUBTYPE: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e),
    pid: 8,
};

/// Wrapper because of that stupid decision to remove `Send` and `Sync` from raw pointers.
#[derive(Clone)]
struct IAudioClientWrapper(Audio::IAudioClient);
unsafe impl Send for IAudioClientWrapper {}
unsafe impl Sync for IAudioClientWrapper {}

/// An opaque type that identifies an end point.
#[derive(Clone)]
pub struct Device {
    device: Audio::IMMDevice,
    /// We cache an uninitialized `IAudioClient` so that we can call functions from it without
    /// having to create/destroy audio clients all the time.
    future_audio_client: Arc<Mutex<Option<IAudioClientWrapper>>>, // TODO: add NonZero around the ptr
}

impl DeviceTrait for Device {
    type SupportedInputConfigs = SupportedInputConfigs;
    type SupportedOutputConfigs = SupportedOutputConfigs;
    type Stream = Stream;

    fn description(&self) -> Result<DeviceDescription, DeviceNameError> {
        Device::description(self)
    }

    fn id(&self) -> Result<DeviceId, DeviceIdError> {
        Device::id(self)
    }

    fn supports_input(&self) -> bool {
        self.data_flow() == Audio::eCapture
    }

    fn supports_output(&self) -> bool {
        self.data_flow() == Audio::eRender
    }

    fn supported_input_configs(
        &self,
    ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError> {
        Device::supported_input_configs(self)
    }

    fn supported_output_configs(
        &self,
    ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError> {
        Device::supported_output_configs(self)
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_input_config(self)
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        Device::default_output_config(self)
    }

    fn build_input_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&Data, &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_input_stream_raw_inner(config, sample_format)?;
        Ok(Stream::new_input(
            stream_inner,
            data_callback,
            error_callback,
        ))
    }

    fn build_output_stream_raw<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        data_callback: D,
        error_callback: E,
        _timeout: Option<Duration>,
    ) -> Result<Self::Stream, BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let stream_inner = self.build_output_stream_raw_inner(config, sample_format)?;
        Ok(Stream::new_output(
            stream_inner,
            data_callback,
            error_callback,
        ))
    }
}

struct Endpoint {
    endpoint: Audio::IMMEndpoint,
}

// Use RAII to make sure CoTaskMemFree is called when we are responsible for freeing.
struct WaveFormatExPtr(*mut Audio::WAVEFORMATEX);

impl Drop for WaveFormatExPtr {
    fn drop(&mut self) {
        unsafe {
            Com::CoTaskMemFree(Some(self.0 as *mut _));
        }
    }
}

unsafe fn immendpoint_from_immdevice(device: Audio::IMMDevice) -> Audio::IMMEndpoint {
    device
        .cast::<Audio::IMMEndpoint>()
        .expect("could not query IMMDevice interface for IMMEndpoint")
}

unsafe fn data_flow_from_immendpoint(endpoint: &Audio::IMMEndpoint) -> Audio::EDataFlow {
    endpoint
        .GetDataFlow()
        .expect("could not get endpoint data_flow")
}

// Given the audio client and format, returns whether or not the format is supported.
pub unsafe fn is_format_supported(
    client: &Audio::IAudioClient,
    waveformatex_ptr: *const Audio::WAVEFORMATEX,
) -> Result<bool, SupportedStreamConfigsError> {
    // Check if the given format is supported.
    let mut closest_waveformatex_ptr: *mut Audio::WAVEFORMATEX = ptr::null_mut();

    let result = client.IsFormatSupported(
        Audio::AUDCLNT_SHAREMODE_SHARED,
        waveformatex_ptr,
        Some(&mut closest_waveformatex_ptr as *mut _),
    );

    if !closest_waveformatex_ptr.is_null() {
        Com::CoTaskMemFree(Some(closest_waveformatex_ptr as *mut std::ffi::c_void));
    }

    // `IsFormatSupported` can return `S_FALSE` (which means that a compatible format
    // has been found, but not an exact match) so we also treat this as unsupported.
    match result {
        Audio::AUDCLNT_E_DEVICE_INVALIDATED => Err(SupportedStreamConfigsError::DeviceNotAvailable),
        r if r.is_err() => Ok(false),
        Foundation::S_FALSE => Ok(false),
        _ => Ok(true),
    }
}

// Get a cpal Format from a WAVEFORMATEX.
unsafe fn format_from_waveformatex_ptr(
    waveformatex_ptr: *const Audio::WAVEFORMATEX,
    audio_client: &Audio::IAudioClient,
) -> Option<SupportedStreamConfig> {
    fn cmp_guid(a: &GUID, b: &GUID) -> bool {
        (a.data1, a.data2, a.data3, a.data4) == (b.data1, b.data2, b.data3, b.data4)
    }
    let sample_format = match (
        (*waveformatex_ptr).wBitsPerSample,
        (*waveformatex_ptr).wFormatTag as u32,
    ) {
        (8, Audio::WAVE_FORMAT_PCM) => SampleFormat::U8,
        (16, Audio::WAVE_FORMAT_PCM) => SampleFormat::I16,
        (32, Multimedia::WAVE_FORMAT_IEEE_FLOAT) => SampleFormat::F32,
        (n_bits, KernelStreaming::WAVE_FORMAT_EXTENSIBLE) => {
            let waveformatextensible_ptr = waveformatex_ptr as *const Audio::WAVEFORMATEXTENSIBLE;
            let sub = (*waveformatextensible_ptr).SubFormat;

            if cmp_guid(&sub, &KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM) {
                match n_bits {
                    8 => SampleFormat::U8,
                    16 => SampleFormat::I16,
                    24 => SampleFormat::I24,
                    32 => SampleFormat::I32,
                    64 => SampleFormat::I64,
                    _ => return None,
                }
            } else if n_bits == 32 && cmp_guid(&sub, &Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) {
                SampleFormat::F32
            } else {
                return None;
            }
        }
        // Unknown data format returned by GetMixFormat.
        _ => return None,
    };

    let sample_rate = (*waveformatex_ptr).nSamplesPerSec;

    // GetBufferSizeLimits is only used for Hardware-Offloaded Audio
    // Processing, which was added in Windows 8, which places hardware
    // limits on the size of the audio buffer. If the sound system
    // *isn't* using offloaded audio, we're using a software audio
    // processing stack and have pretty much free rein to set buffer
    // size.
    //
    // In software audio stacks GetBufferSizeLimits returns
    // AUDCLNT_E_OFFLOAD_MODE_ONLY.
    //
    // https://docs.microsoft.com/en-us/windows-hardware/drivers/audio/hardware-offloaded-audio-processing
    let (mut min_buffer_duration, mut max_buffer_duration) = (0, 0);
    let buffer_size_is_limited = audio_client
        .cast::<Audio::IAudioClient2>()
        .and_then(|audio_client| {
            audio_client.GetBufferSizeLimits(
                waveformatex_ptr,
                true,
                &mut min_buffer_duration,
                &mut max_buffer_duration,
            )
        })
        .is_ok();
    let buffer_size = if buffer_size_is_limited {
        SupportedBufferSize::Range {
            min: buffer_duration_to_frames(min_buffer_duration, sample_rate),
            max: buffer_duration_to_frames(max_buffer_duration, sample_rate),
        }
    } else {
        SupportedBufferSize::Range {
            min: 0,
            max: u32::MAX,
        }
    };

    let format = SupportedStreamConfig {
        channels: (*waveformatex_ptr).nChannels as _,
        sample_rate,
        buffer_size,
        sample_format,
    };
    Some(format)
}

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

/// Maps PKEY_AudioEndpoint_JackSubType GUID to InterfaceType.
///
/// The JackSubType property contains a KS node type GUID string from Ksmedia.h
/// that specifies the physical connector type.
fn jacksubtype_to_interface_type(guid_str: &str) -> Option<crate::InterfaceType> {
    let guid_upper = guid_str.to_uppercase();
    let typ = match guid_upper.as_str() {
        "{D9E55EA0-0C89-4692-84FF-EB3C4B0D172F}" => InterfaceType::Hdmi,
        "{E47E4031-3EA6-418D-8F9B-B73843CCB2AD}" => InterfaceType::DisplayPort,
        "{DFF21CE1-F70F-11D0-B917-00A0C9223196}" => InterfaceType::Spdif,
        _ => return None,
    };

    Some(typ)
}

/// Maps WASAPI FormFactor values to DeviceType and optionally InterfaceType.
fn form_factor_to_types(form_factor: u32) -> (crate::DeviceType, Option<crate::InterfaceType>) {
    match form_factor {
        0 => (DeviceType::Unknown, Some(InterfaceType::Network)), // RemoteNetworkDevice
        1 => (DeviceType::Speaker, None),                         // Speakers
        2 => (DeviceType::Unknown, Some(InterfaceType::Line)),    // LineLevel
        3 => (DeviceType::Headphones, None),                      // Headphones
        4 => (DeviceType::Microphone, None),                      // Microphone
        5 => (DeviceType::Headset, None),                         // Headset
        6 => (DeviceType::Handset, None),                         // Handset
        7 => (DeviceType::Unknown, None),                         // UnknownDigitalPassthrough
        8 => (DeviceType::Unknown, Some(InterfaceType::Spdif)),   // SPDIF
        9 => (DeviceType::Unknown, Some(InterfaceType::Hdmi)),    // DigitalAudioDisplayDevice
        _ => (DeviceType::Unknown, None), // UnknownFormFactor or future values
    }
}

/// Maps WASAPI EnumeratorName to InterfaceType.
fn enumerator_to_interface_type(enumerator: &str) -> Option<crate::InterfaceType> {
    let typ = match enumerator.to_uppercase().as_str() {
        "HDAUDIO" => InterfaceType::BuiltIn,
        "USB" => InterfaceType::Usb,
        "BTHENUM" => InterfaceType::Bluetooth,
        "MMDEVAPI" | "SW" => InterfaceType::Virtual,
        _ => return None,
    };
    Some(typ)
}

impl Device {
    pub fn description(&self) -> Result<DeviceDescription, DeviceNameError> {
        unsafe {
            // Open the device's property store.
            let property_store = self
                .device
                .OpenPropertyStore(STGM_READ)
                .expect("could not open property store");

            // Query all available properties
            let friendly_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_FriendlyName as *const _ as *const _,
            );

            let device_desc = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_DeviceDesc as *const _ as *const _,
            );

            let interface_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_DeviceInterface_FriendlyName as *const _ as *const _,
            );

            let enumerator_name = get_property_string(
                &property_store,
                &Properties::DEVPKEY_Device_EnumeratorName as *const _ as *const _,
            );

            let form_factor = get_property_u32(
                &property_store,
                &PKEY_AUDIOENDPOINT_FORMFACTOR as *const _ as *const _,
            );

            let jack_subtype = get_property_string(
                &property_store,
                &PKEY_AUDIOENDPOINT_JACKSUBTYPE as *const _ as *const _,
            );

            // Prefer DeviceDesc for name, fall back to FriendlyName
            let name = device_desc
                .clone()
                .or(friendly_name.clone())
                .ok_or_else(|| DeviceNameError::BackendSpecific {
                    err: BackendSpecificError {
                        description: "failed to retrieve device name".to_string(),
                    },
                })?;

            // Get direction from data flow (eCapture = Input, eRender = Output)
            let direction = self.data_flow().into();

            // Determine device_type and initial interface_type from FormFactor
            let (device_type, mut interface_type) = form_factor
                .map(form_factor_to_types)
                .unwrap_or((crate::DeviceType::Unknown, None));

            // Override interface_type from EnumeratorName if available
            if let Some(ref enumerator) = enumerator_name {
                if let Some(itype) = enumerator_to_interface_type(enumerator) {
                    interface_type = Some(itype);
                }
            }

            // JackSubType has highest priority for interface_type
            if let Some(ref jack_guid) = jack_subtype {
                if let Some(itype) = jacksubtype_to_interface_type(jack_guid) {
                    interface_type = Some(itype);
                }
            }

            let mut builder = DeviceDescriptionBuilder::new(name)
                .direction(direction)
                .device_type(device_type);

            if let Some(itype) = interface_type {
                builder = builder.interface_type(itype);
            }

            // Add interface name to driver field if available
            if let Some(iface_name) = interface_name {
                builder = builder.driver(iface_name);
            }

            // Add FriendlyName to extended if different from the name we used
            if let Some(fname) = friendly_name {
                if device_desc.is_some() && Some(&fname) != device_desc.as_ref() {
                    builder = builder.add_extended_line(fname);
                }
            }

            Ok(builder.build())
        }
    }

    fn id(&self) -> Result<DeviceId, DeviceIdError> {
        unsafe {
            match self.device.GetId() {
                Ok(pwstr) => match pwstr.to_string() {
                    Ok(id_str) => Ok(DeviceId(crate::platform::HostId::Wasapi, id_str)),
                    Err(e) => Err(DeviceIdError::BackendSpecific {
                        err: BackendSpecificError {
                            description: format!("Failed to convert device ID to string: {}", e),
                        },
                    }),
                },
                Err(e) => Err(DeviceIdError::BackendSpecific { err: e.into() }),
            }
        }
    }

    fn from_immdevice(device: Audio::IMMDevice) -> Self {
        Device {
            device,
            future_audio_client: Arc::new(Mutex::new(None)),
        }
    }

    pub fn immdevice(&self) -> &Audio::IMMDevice {
        &self.device
    }

    /// Ensures that `future_audio_client` contains a `Some` and returns a locked mutex to it.
    fn ensure_future_audio_client(
        &self,
    ) -> Result<MutexGuard<'_, Option<IAudioClientWrapper>>, windows::core::Error> {
        let mut lock = self.future_audio_client.lock().unwrap();
        if lock.is_some() {
            return Ok(lock);
        }

        let audio_client: Audio::IAudioClient = unsafe {
            // can fail if the device has been disconnected since we enumerated it, or if
            // the device doesn't support playback for some reason
            self.device.Activate(Com::CLSCTX_ALL, None)?
        };

        *lock = Some(IAudioClientWrapper(audio_client));
        Ok(lock)
    }

    /// Returns an uninitialized `IAudioClient`.
    pub(crate) fn build_audioclient(&self) -> Result<Audio::IAudioClient, windows::core::Error> {
        let mut lock = self.ensure_future_audio_client()?;
        Ok(lock.take().unwrap().0)
    }

    // There is no way to query the list of all formats that are supported by the
    // audio processor, so instead we just trial some commonly supported formats.
    //
    // Common formats are trialed by first getting the default format (returned via
    // `GetMixFormat`) and then mutating that format with common sample rates and
    // querying them via `IsFormatSupported`.
    //
    // When calling `IsFormatSupported` with the shared-mode audio engine, only the default
    // number of channels seems to be supported. Any, more or less returns an invalid
    // parameter error. Thus, we just assume that the default number of channels is the only
    // number supported.
    fn supported_formats(&self) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        // initializing COM because we call `CoTaskMemFree` to release the format.
        com::com_initialized();

        // Retrieve the `IAudioClient`.
        let lock = match self.ensure_future_audio_client() {
            Ok(lock) => lock,
            Err(ref e) if e.code() == Audio::AUDCLNT_E_DEVICE_INVALIDATED => {
                return Err(SupportedStreamConfigsError::DeviceNotAvailable)
            }
            Err(e) => {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
        };
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            // Retrieve the pointer to the default WAVEFORMATEX.
            let default_waveformatex_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .map_err(windows_err_to_cpal_err::<SupportedStreamConfigsError>)?;

            // If the default format can't succeed we have no hope of finding other formats.
            if !is_format_supported(client, default_waveformatex_ptr.0)? {
                let description =
                    "Could not determine support for default `WAVEFORMATEX`".to_string();
                let err = BackendSpecificError { description };
                return Err(err.into());
            }

            let format = match format_from_waveformatex_ptr(default_waveformatex_ptr.0, client) {
                Some(fmt) => fmt,
                None => {
                    let description =
                        "could not create a `cpal::SupportedStreamConfig` from a `WAVEFORMATEX`"
                            .to_string();
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            let mut sample_rates: Vec<SampleRate> = COMMON_SAMPLE_RATES.to_vec();

            if !sample_rates.contains(&format.sample_rate) {
                sample_rates.push(format.sample_rate)
            }

            let mut supported_formats = Vec::new();

            for sample_rate in sample_rates {
                for sample_format in [
                    SampleFormat::U8,
                    SampleFormat::I16,
                    SampleFormat::I24,
                    SampleFormat::U24,
                    SampleFormat::I32,
                    SampleFormat::I64,
                    SampleFormat::F32,
                ] {
                    if let Some(waveformat) = config_to_waveformatextensible(
                        &StreamConfig {
                            channels: format.channels,
                            sample_rate,
                            buffer_size: BufferSize::Default,
                        },
                        sample_format,
                    ) {
                        if is_format_supported(
                            client,
                            &waveformat.Format as *const Audio::WAVEFORMATEX,
                        )? {
                            supported_formats.push(SupportedStreamConfigRange {
                                channels: format.channels,
                                min_sample_rate: sample_rate,
                                max_sample_rate: sample_rate,
                                buffer_size: format.buffer_size,
                                sample_format,
                            })
                        }
                    }
                }
            }
            Ok(supported_formats.into_iter())
        }
    }

    pub fn supported_input_configs(
        &self,
    ) -> Result<SupportedInputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == Audio::eCapture {
            self.supported_formats()
        // If it's an output device, assume no input formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    pub fn supported_output_configs(
        &self,
    ) -> Result<SupportedOutputConfigs, SupportedStreamConfigsError> {
        if self.data_flow() == Audio::eRender {
            self.supported_formats()
        // If it's an input device, assume no output formats.
        } else {
            Ok(vec![].into_iter())
        }
    }

    // We always create voices in shared mode, therefore all samples go through an audio
    // processor to mix them together.
    //
    // One format is guaranteed to be supported, the one returned by `GetMixFormat`.
    fn default_format(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        // initializing COM because we call `CoTaskMemFree`
        com::com_initialized();

        let lock = match self.ensure_future_audio_client() {
            Ok(lock) => lock,
            Err(ref e) if e.code() == Audio::AUDCLNT_E_DEVICE_INVALIDATED => {
                return Err(DefaultStreamConfigError::DeviceNotAvailable)
            }
            Err(e) => {
                let description = format!("{}", e);
                let err = BackendSpecificError { description };
                return Err(err.into());
            }
        };
        let client = &lock.as_ref().unwrap().0;

        unsafe {
            let format_ptr = client
                .GetMixFormat()
                .map(WaveFormatExPtr)
                .map_err(windows_err_to_cpal_err::<DefaultStreamConfigError>)?;

            format_from_waveformatex_ptr(format_ptr.0, client)
                .ok_or(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub(crate) fn data_flow(&self) -> Audio::EDataFlow {
        let endpoint = Endpoint::from(self.device.clone());
        endpoint.data_flow()
    }

    pub fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        if self.data_flow() == Audio::eCapture {
            self.default_format()
        } else {
            Err(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let data_flow = self.data_flow();
        if data_flow == Audio::eRender {
            self.default_format()
        } else {
            Err(DefaultStreamConfigError::StreamTypeNotSupported)
        }
    }

    pub(crate) fn build_input_stream_raw_inner(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<StreamInner, BuildStreamError> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = match self.build_audioclient() {
                Ok(client) => client,
                Err(ref e) if e.code() == Audio::AUDCLNT_E_DEVICE_INVALIDATED => {
                    return Err(BuildStreamError::DeviceNotAvailable)
                }
                Err(e) => {
                    let description = format!("{}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }
            };

            // Note: Buffer size validation is not needed here - `IAudioClient::Initialize`
            // will return `AUDCLNT_E_BUFFER_SIZE_ERROR` if the buffer size is not supported.
            let buffer_duration = buffer_size_to_duration(&config.buffer_size, config.sample_rate);

            let mut stream_flags = Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK;

            if self.data_flow() == Audio::eRender {
                stream_flags |= Audio::AUDCLNT_STREAMFLAGS_LOOPBACK;
            }

            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                // Ensure the format is supported.
                match super::device::is_format_supported(&audio_client, &format_attempt.Format) {
                    Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                    Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                    _ => (),
                }

                // Finally, initializing the audio client
                let hresult = audio_client.Initialize(
                    share_mode,
                    stream_flags,
                    buffer_duration,
                    0,
                    &format_attempt.Format,
                    None,
                );
                match hresult {
                    Err(ref e) if e.code() == Audio::AUDCLNT_E_DEVICE_INVALIDATED => {
                        return Err(BuildStreamError::DeviceNotAvailable);
                    }
                    Err(e) => {
                        let description = format!("{}", e);
                        let err = BackendSpecificError { description };
                        return Err(err.into());
                    }
                    Ok(()) => (),
                };

                format_attempt.Format
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client
                .GetBufferSize()
                .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;

            // Creating the event that will be signalled whenever we need to submit some samples.
            let event = {
                let event =
                    Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
                        .map_err(|e| {
                            let description = format!("failed to create event: {}", e);
                            let err = BackendSpecificError { description };
                            BuildStreamError::from(err)
                        })?;

                if let Err(e) = audio_client.SetEventHandle(event) {
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                event
            };

            // Building a `IAudioCaptureClient` that will be used to read captured samples.
            let capture_client = audio_client
                .GetService::<Audio::IAudioCaptureClient>()
                .map_err(|e| {
                    windows_err_to_cpal_err_message::<BuildStreamError>(
                        e,
                        "failed to build capture client: ",
                    )
                })?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Capture { capture_client };

            let audio_clock = get_audio_clock(&audio_client)?;

            Ok(StreamInner {
                audio_client,
                audio_clock,
                client_flow,
                event,
                playing: false,
                max_frames_in_buffer,
                bytes_per_frame: waveformatex.nBlockAlign,
                config: config.clone(),
                sample_format,
            })
        }
    }

    /// Query the exact format without initializing or starting an output stream.
    pub fn shared_output_engine_periods(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        match_engine_format: bool,
    ) -> Result<SharedOutputEnginePeriods, BuildStreamError> {
        com::com_initialized();
        let client = self
            .ensure_future_audio_client()
            .map_err(|error| shared_period_error("Activate IAudioClient", error))?;
        let client = &client.as_ref().unwrap().0;
        let format = config_to_waveformatextensible(config, sample_format)
            .ok_or(BuildStreamError::StreamConfigNotSupported)?;
        unsafe {
            let client3 = client
                .cast::<Audio::IAudioClient3>()
                .map_err(|error| shared_period_error("Query IAudioClient3", error))?;
            configure_shared_client(&client3, match_engine_format)?;
            require_exact_shared_format(client, &format.Format)?;
            query_shared_periods(&client3, &format.Format)
        }
    }

    /// Initialize an exact IAudioClient3 period. None selects its reported default.
    /// Unsupported/locked formats and periods are returned as errors, never rounded.
    pub fn build_output_stream_raw_with_shared_period<D, E>(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        period_frames: Option<u32>,
        match_engine_format: bool,
        data_callback: D,
        error_callback: E,
    ) -> Result<(Stream, SharedOutputStreamInfo), BuildStreamError>
    where
        D: FnMut(&mut Data, &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let (inner, info) = self.build_output_stream_initialized(
            config,
            sample_format,
            OutputInitialization::SharedPeriod {
                frames: period_frames,
                match_format: match_engine_format,
            },
        )?;
        Ok((
            Stream::new_output(inner, data_callback, error_callback),
            info.unwrap(),
        ))
    }

    pub(crate) fn build_output_stream_raw_inner(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
    ) -> Result<StreamInner, BuildStreamError> {
        self.build_output_stream_initialized(config, sample_format, OutputInitialization::Legacy)
            .map(|(inner, _)| inner)
    }

    fn build_output_stream_initialized(
        &self,
        config: &StreamConfig,
        sample_format: SampleFormat,
        initialization: OutputInitialization,
    ) -> Result<(StreamInner, Option<SharedOutputStreamInfo>), BuildStreamError> {
        unsafe {
            // Making sure that COM is initialized.
            // It's not actually sure that this is required, but when in doubt do it.
            com::com_initialized();

            // Obtaining a `IAudioClient`.
            let audio_client = self
                .build_audioclient()
                .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;

            // Note: Buffer size validation is not needed here - `IAudioClient::Initialize`
            // will return `AUDCLNT_E_BUFFER_SIZE_ERROR` if the buffer size is not supported.
            let buffer_duration = buffer_size_to_duration(&config.buffer_size, config.sample_rate);

            let mut shared_info = None;
            // Computing the format and initializing the device.
            let waveformatex = {
                let format_attempt = config_to_waveformatextensible(config, sample_format)
                    .ok_or(BuildStreamError::StreamConfigNotSupported)?;
                let share_mode = Audio::AUDCLNT_SHAREMODE_SHARED;

                match initialization {
                    OutputInitialization::Legacy => {
                        match super::device::is_format_supported(
                            &audio_client,
                            &format_attempt.Format,
                        ) {
                            Ok(false) => return Err(BuildStreamError::StreamConfigNotSupported),
                            Err(_) => return Err(BuildStreamError::DeviceNotAvailable),
                            _ => (),
                        }
                        audio_client
                            .Initialize(
                                share_mode,
                                Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                buffer_duration,
                                0,
                                &format_attempt.Format,
                                None,
                            )
                            .map_err(windows_err_to_cpal_err::<BuildStreamError>)?;
                    }
                    OutputInitialization::SharedPeriod {
                        frames: requested,
                        match_format,
                    } => {
                        let client3 = audio_client
                            .cast::<Audio::IAudioClient3>()
                            .map_err(|error| shared_period_error("Query IAudioClient3", error))?;
                        configure_shared_client(&client3, match_format)?;
                        require_exact_shared_format(&audio_client, &format_attempt.Format)?;
                        let periods = query_shared_periods(&client3, &format_attempt.Format)?;
                        let frames = periods.select(requested)?;
                        client3
                            .InitializeSharedAudioStream(
                                Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                frames,
                                &format_attempt.Format,
                                None,
                            )
                            .map_err(|error| {
                                shared_period_error("InitializeSharedAudioStream", error)
                            })?;
                        let mut current_format = ptr::null_mut();
                        let mut current_frames = 0;
                        let current_result = client3.GetCurrentSharedModeEnginePeriod(
                            &mut current_format,
                            &mut current_frames,
                        );
                        let _current_format_owner = WaveFormatExPtr(current_format);
                        let observed_format = if current_result.is_ok()
                            && !current_format.is_null()
                            && current_frames != 0
                        {
                            format_from_waveformatex_ptr(current_format, &audio_client)
                        } else {
                            None
                        };
                        let current_error = match current_result {
                            Err(error) => Some(error.to_string()),
                            Ok(()) if observed_format.is_none() => Some(
                                "Current shared engine returned an unavailable format or period"
                                    .into(),
                            ),
                            Ok(()) => None,
                        };
                        let latency = audio_client
                            .GetStreamLatency()
                            .map_err(|error| shared_period_error("GetStreamLatency", error))?;
                        if latency < 0 {
                            return Err(BackendSpecificError {
                                description: "GetStreamLatency returned a negative latency".into(),
                            }
                            .into());
                        }
                        shared_info = Some(SharedOutputStreamInfo {
                            periods,
                            initialized_period_frames: frames,
                            buffer_capacity_frames: 0,
                            reported_latency_100ns: latency,
                            current_period_frames: observed_format.as_ref().map(|_| current_frames),
                            current_sample_rate_hz: observed_format
                                .as_ref()
                                .map(|format| format.sample_rate()),
                            current_channels: observed_format
                                .as_ref()
                                .map(|format| format.channels()),
                            current_sample_format: observed_format
                                .as_ref()
                                .map(|format| format.sample_format()),
                            current_period_error: current_error,
                        });
                    }
                }

                format_attempt.Format
            };

            // Creating the event that will be signalled whenever we need to submit some samples.
            let mut event_owner = PendingEvent(None);
            let event = {
                let event =
                    Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
                        .map_err(|e| {
                            let description = format!("failed to create event: {}", e);
                            let err = BackendSpecificError { description };
                            BuildStreamError::from(err)
                        })?;

                event_owner.0 = Some(event);
                if let Err(e) = audio_client.SetEventHandle(event) {
                    let description = format!("failed to call SetEventHandle: {}", e);
                    let err = BackendSpecificError { description };
                    return Err(err.into());
                }

                event
            };

            // obtaining the size of the samples buffer in number of frames
            let max_frames_in_buffer = audio_client.GetBufferSize().map_err(|e| {
                windows_err_to_cpal_err_message::<BuildStreamError>(
                    e,
                    "failed to obtain buffer size: ",
                )
            })?;

            // Building a `IAudioRenderClient` that will be used to fill the samples buffer.
            let render_client = audio_client
                .GetService::<IAudioRenderClient>()
                .map_err(|e| {
                    windows_err_to_cpal_err_message::<BuildStreamError>(
                        e,
                        "failed to build render client: ",
                    )
                })?;

            // Once we built the `StreamInner`, we add a command that will be picked up by the
            // `run()` method and added to the `RunContext`.
            let client_flow = AudioClientFlow::Render { render_client };

            let audio_clock = get_audio_clock(&audio_client)?;

            if let Some(info) = shared_info.as_mut() {
                info.buffer_capacity_frames = max_frames_in_buffer;
            }
            event_owner.0 = None;
            Ok((
                StreamInner {
                    audio_client,
                    audio_clock,
                    client_flow,
                    event,
                    playing: false,
                    max_frames_in_buffer,
                    bytes_per_frame: waveformatex.nBlockAlign,
                    config: config.clone(),
                    sample_format,
                },
                shared_info,
            ))
        }
    }
}

impl PartialEq for Device {
    fn eq(&self, other: &Device) -> bool {
        // Use case: In order to check whether the default device has changed
        // the client code might need to compare the previous default device with the current one.
        // The pointer comparison (`self.device == other.device`) don't work there,
        // because the pointers are different even when the default device stays the same.
        //
        // In this code section we're trying to use the GetId method for the device comparison, cf.
        // https://docs.microsoft.com/en-us/windows/desktop/api/mmdeviceapi/nf-mmdeviceapi-immdevice-getid
        unsafe {
            struct IdRAII(windows::core::PWSTR);
            /// RAII for device IDs.
            impl Drop for IdRAII {
                fn drop(&mut self) {
                    unsafe { Com::CoTaskMemFree(Some(self.0 .0 as *mut _)) }
                }
            }
            // GetId only fails with E_OUTOFMEMORY and if it does, we're probably dead already.
            // Plus it won't do to change the device comparison logic unexpectedly.
            let id1 = self.device.GetId().expect("cpal: GetId failure");
            let id1 = IdRAII(id1);
            let id2 = other.device.GetId().expect("cpal: GetId failure");
            let id2 = IdRAII(id2);
            // 16-bit null-terminated comparison.
            let mut offset = 0;
            loop {
                let w1: u16 = *(id1.0).0.offset(offset);
                let w2: u16 = *(id2.0).0.offset(offset);
                if w1 == 0 && w2 == 0 {
                    return true;
                }
                if w1 != w2 {
                    return false;
                }
                offset += 1;
            }
        }
    }
}

impl Eq for Device {}

impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the device ID for consistency with PartialEq
        // SAFETY: GetId only fails with E_OUTOFMEMORY, which is unrecoverable.
        // We need consistent hash/eq behavior.
        unsafe {
            use windows::Win32::System::Com;

            struct IdRAII(windows::core::PWSTR);
            impl Drop for IdRAII {
                fn drop(&mut self) {
                    unsafe { Com::CoTaskMemFree(Some(self.0 .0 as *mut _)) }
                }
            }

            let id = self.device.GetId().expect("cpal: GetId failure");
            let id = IdRAII(id);

            // Hash the 16-bit null-terminated string
            let mut offset = 0;
            loop {
                let w: u16 = *(id.0).0.offset(offset);
                if w == 0 {
                    break;
                }
                w.hash(state);
                offset += 1;
            }
        }
    }
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Device")
            .field("device", &self.device)
            .field("description", &self.description())
            .finish()
    }
}

impl From<Audio::IMMDevice> for Endpoint {
    fn from(device: Audio::IMMDevice) -> Self {
        unsafe {
            let endpoint = immendpoint_from_immdevice(device);
            Endpoint { endpoint }
        }
    }
}

impl Endpoint {
    fn data_flow(&self) -> Audio::EDataFlow {
        unsafe { data_flow_from_immendpoint(&self.endpoint) }
    }
}

static ENUMERATOR: OnceLock<Enumerator> = OnceLock::new();

fn get_enumerator() -> &'static Enumerator {
    ENUMERATOR.get_or_init(|| {
        // COM initialization is thread local, but we only need to have COM initialized in the
        // thread we create the objects in
        com::com_initialized();

        // building the devices enumerator object
        unsafe {
            let enumerator = Com::CoCreateInstance::<_, Audio::IMMDeviceEnumerator>(
                &Audio::MMDeviceEnumerator,
                None,
                Com::CLSCTX_ALL,
            )
            .unwrap();

            Enumerator(enumerator)
        }
    })
}

// Helper function to query a DWORD property from a WASAPI device property store
unsafe fn get_property_u32(
    property_store: &IPropertyStore,
    property_key: *const PROPERTYKEY,
) -> Option<u32> {
    let mut property_value = property_store.GetValue(property_key).ok()?;
    let prop_variant = &property_value.Anonymous.Anonymous;

    // Check if it's a UI4 (unsigned 32-bit integer)
    if prop_variant.vt != VT_UI4 {
        return None;
    }

    let value = *(&prop_variant.Anonymous as *const _ as *const u32);

    // Clean up the property
    StructuredStorage::PropVariantClear(&mut property_value).ok();

    Some(value)
}

// Helper function to query a string property from a WASAPI device property store
unsafe fn get_property_string(
    property_store: &IPropertyStore,
    property_key: *const PROPERTYKEY,
) -> Option<String> {
    let mut property_value = property_store.GetValue(property_key).ok()?;
    let prop_variant = &property_value.Anonymous.Anonymous;

    // Read the string from the union data field, expecting a *const u16.
    if prop_variant.vt != VT_LPWSTR {
        return None;
    }
    let ptr_utf16 = *(&prop_variant.Anonymous as *const _ as *const *const u16);

    // Find the length of the null-terminated string with a safety limit
    const MAX_STRING_LEN: usize = 32768; // 32K characters should be more than enough
    let mut len = 0;
    while len < MAX_STRING_LEN && *ptr_utf16.add(len) != 0 {
        len += 1;
    }

    // If we hit the limit, the string is likely malformed (not null-terminated)
    if len >= MAX_STRING_LEN {
        return None;
    }

    // Create the utf16 slice and convert it into a string.
    let string_slice = slice::from_raw_parts(ptr_utf16, len);
    let os_string: OsString = OsStringExt::from_wide(string_slice);
    let result = match os_string.into_string() {
        Ok(string) => Some(string),
        Err(os_string) => Some(os_string.to_string_lossy().into()),
    };

    // Clean up the property.
    StructuredStorage::PropVariantClear(&mut property_value).ok();

    result
}

/// Send/Sync wrapper around `IMMDeviceEnumerator`.
struct Enumerator(Audio::IMMDeviceEnumerator);

unsafe impl Send for Enumerator {}
unsafe impl Sync for Enumerator {}

/// WASAPI implementation for `Devices`.
pub struct Devices {
    collection: Audio::IMMDeviceCollection,
    total_count: u32,
    next_item: u32,
}

impl Devices {
    pub fn new() -> Result<Self, DevicesError> {
        unsafe {
            // can fail because of wrong parameters (should never happen) or out of memory
            let collection = get_enumerator()
                .0
                .EnumAudioEndpoints(Audio::eAll, Audio::DEVICE_STATE_ACTIVE)
                .map_err(BackendSpecificError::from)?;

            let count = collection.GetCount().map_err(BackendSpecificError::from)?;

            Ok(Devices {
                collection,
                total_count: count,
                next_item: 0,
            })
        }
    }
}

unsafe impl Send for Devices {}
unsafe impl Sync for Devices {}

impl Iterator for Devices {
    type Item = Device;

    fn next(&mut self) -> Option<Device> {
        if self.next_item >= self.total_count {
            return None;
        }

        unsafe {
            let device = self.collection.Item(self.next_item).unwrap();
            self.next_item += 1;
            Some(Device::from_immdevice(device))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let num = self.total_count - self.next_item;
        let num = num as usize;
        (num, Some(num))
    }
}

fn default_device(data_flow: Audio::EDataFlow) -> Option<Device> {
    unsafe {
        let device = get_enumerator()
            .0
            .GetDefaultAudioEndpoint(data_flow, Audio::eConsole)
            .ok()?;
        // TODO: check specifically for `E_NOTFOUND`, and panic otherwise
        Some(Device::from_immdevice(device))
    }
}

pub fn default_input_device() -> Option<Device> {
    default_device(Audio::eCapture)
}

pub fn default_output_device() -> Option<Device> {
    default_device(Audio::eRender)
}

/// Get the audio clock used to produce `StreamInstant`s.
unsafe fn get_audio_clock(
    audio_client: &Audio::IAudioClient,
) -> Result<Audio::IAudioClock, BuildStreamError> {
    audio_client
        .GetService::<Audio::IAudioClock>()
        .map_err(|e| {
            windows_err_to_cpal_err_message::<BuildStreamError>(e, "failed to build audio clock: ")
        })
}

// Turns a `Format` into a `WAVEFORMATEXTENSIBLE`.
//
// Returns `None` if the WAVEFORMATEXTENSIBLE does not support the given format.
fn config_to_waveformatextensible(
    config: &StreamConfig,
    sample_format: SampleFormat,
) -> Option<Audio::WAVEFORMATEXTENSIBLE> {
    let format_tag = match sample_format {
        SampleFormat::U8 | SampleFormat::I16 => Audio::WAVE_FORMAT_PCM,

        SampleFormat::I24
        | SampleFormat::U24
        | SampleFormat::I32
        | SampleFormat::I64
        | SampleFormat::F32 => KernelStreaming::WAVE_FORMAT_EXTENSIBLE,

        _ => return None,
    };
    let channels = config.channels;
    let sample_rate = config.sample_rate;
    let sample_bytes = sample_format.sample_size() as u16;
    let avg_bytes_per_sec = u32::from(channels) * sample_rate * u32::from(sample_bytes);
    let block_align = channels * sample_bytes;
    let bits_per_sample = match sample_format {
        // 24-bit formats use 32-bit storage but only 24 valid bits
        SampleFormat::I24 | SampleFormat::U24 => 24,
        _ => 8 * sample_bytes,
    };

    let cb_size = if format_tag == Audio::WAVE_FORMAT_PCM {
        0
    } else {
        let extensible_size = mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>();
        let ex_size = mem::size_of::<Audio::WAVEFORMATEX>();
        (extensible_size - ex_size) as u16
    };

    let waveformatex = Audio::WAVEFORMATEX {
        wFormatTag: format_tag as u16,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: avg_bytes_per_sec,
        nBlockAlign: block_align,
        wBitsPerSample: bits_per_sample,
        cbSize: cb_size,
    };

    // CPAL does not care about speaker positions, so pass audio right through.
    let channel_mask = KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT;

    let sub_format = match sample_format {
        SampleFormat::U8
        | SampleFormat::I16
        | SampleFormat::I24
        | SampleFormat::U24
        | SampleFormat::I32
        | SampleFormat::I64 => KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM,

        SampleFormat::F32 => Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
        _ => return None,
    };

    let waveformatextensible = Audio::WAVEFORMATEXTENSIBLE {
        Format: waveformatex,
        Samples: Audio::WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: channel_mask,
        SubFormat: sub_format,
    };

    Some(waveformatextensible)
}

fn buffer_size_to_duration(buffer_size: &BufferSize, sample_rate: u32) -> i64 {
    match buffer_size {
        BufferSize::Fixed(frames) => *frames as i64 * (1_000_000_000 / 100) / sample_rate as i64,
        BufferSize::Default => 0,
    }
}

fn buffer_duration_to_frames(buffer_duration: i64, sample_rate: u32) -> FrameCount {
    (buffer_duration * sample_rate as i64 * 100 / 1_000_000_000) as FrameCount
}

#[cfg(test)]
mod shared_period_contract_tests {
    use super::*;

    #[test]
    fn auto_uses_reported_default_and_fixed_periods_are_never_rounded() {
        let periods = SharedOutputEnginePeriods {
            default_frames: 480,
            fundamental_frames: 48,
            min_frames: 96,
            max_frames: 960,
        };
        assert_eq!(periods.select(None).unwrap(), 480);
        assert_eq!(periods.select(Some(96)).unwrap(), 96);
        assert_eq!(periods.select(Some(960)).unwrap(), 960);
        for frames in [0, 48, 95, 97, 100, 961, u32::MAX] {
            assert!(periods.select(Some(frames)).is_err(), "{frames}");
        }
        for invalid in [
            SharedOutputEnginePeriods {
                fundamental_frames: 0,
                ..periods
            },
            SharedOutputEnginePeriods {
                default_frames: 481,
                ..periods
            },
            SharedOutputEnginePeriods {
                min_frames: 0,
                ..periods
            },
            SharedOutputEnginePeriods {
                max_frames: 95,
                ..periods
            },
        ] {
            assert!(invalid.select(None).is_err());
            assert!(invalid.select(Some(96)).is_err());
        }
    }

    #[test]
    fn exact_format_distinguishes_closest_format_from_original_hresult() {
        assert!(exact_shared_format_result(Foundation::S_OK).is_ok());
        for result in [Foundation::S_FALSE, Audio::AUDCLNT_E_UNSUPPORTED_FORMAT] {
            assert!(matches!(
                exact_shared_format_result(result),
                Err(BuildStreamError::StreamConfigNotSupported)
            ));
        }
        let failure = windows::core::HRESULT(0x8007_0057_u32 as i32);
        let error = exact_shared_format_result(failure).unwrap_err().to_string();
        assert!(error.contains("IsFormatSupported") && error.contains("80070057"));
    }
}
