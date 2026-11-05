//! Requested output intent and control-thread discovery. No device runs here.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct OutputOptions {
    pub backend: String,
    pub device_id: Option<String>,
    pub sample_rate_hz: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub left_channel: Option<u32>,
    pub right_channel: Option<u32>,
    // Internal captured rollback policy, separate from Human Auto preference.
    #[cfg(windows)]
    #[serde(skip)]
    pub wasapi_match_engine_format: Option<bool>,
    #[cfg(all(windows, test, feature = "asio-contract-tests"))]
    #[serde(skip)]
    pub test_driver: Option<std::sync::Arc<super::asio_output::test_driver::Factory>>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
            device_id: None,
            sample_rate_hz: None,
            buffer_frames: None,
            left_channel: None,
            right_channel: None,
            #[cfg(windows)]
            wasapi_match_engine_format: None,
            #[cfg(all(windows, test, feature = "asio-contract-tests"))]
            test_driver: None,
        }
    }
}

impl OutputOptions {
    pub fn parse(json: Option<&str>) -> Result<Self, String> {
        let options: Self = match json {
            Some(value) => serde_json::from_str(value).map_err(|error| error.to_string())?,
            None => Self::default(),
        };
        if options.backend.is_empty()
            || options.backend.len() > 32
            || !options.backend.as_bytes()[0].is_ascii_lowercase()
            || !options.backend.bytes().all(|value| {
                value.is_ascii_lowercase()
                    || value.is_ascii_digit()
                    || value == b'_'
                    || value == b'-'
            })
        {
            return Err("Invalid output backend identifier".into());
        }
        if options
            .device_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 4096)
            || options.sample_rate_hz == Some(0)
            || options
                .buffer_frames
                .is_some_and(|frames| frames == 0 || frames > i32::MAX as u32)
            || options
                .left_channel
                .is_some_and(|channel| channel > i32::MAX as u32)
            || options
                .right_channel
                .is_some_and(|channel| channel > i32::MAX as u32)
        {
            return Err("Invalid output device, rate, buffer or channel".into());
        }
        match (options.left_channel, options.right_channel) {
            (None, None) => {}
            (Some(left), Some(right)) if left != right => {}
            _ => return Err("Choose two distinct physical outputs or automatic routing".into()),
        }
        Ok(options)
    }

    pub fn physical_pair(&self, channels: usize) -> Result<(usize, usize), String> {
        let left = self.left_channel.unwrap_or(0) as usize;
        let right = self
            .right_channel
            .unwrap_or(if channels == 1 { 0 } else { 1 }) as usize;
        if channels == 0 || left >= channels || right >= channels {
            return Err("Selected physical output is unavailable".into());
        }
        Ok((left, right))
    }

    /// Resolve acquisition without changing the persisted/requested preference.
    #[cfg(windows)]
    pub fn resolve_windows_backend(
        &self,
        discover: impl FnOnce() -> OutputBackend,
    ) -> Result<Self, String> {
        if self.backend != "auto" {
            return Ok(self.clone());
        }
        let mut resolved = self.clone();
        // Auto IDs saved by the former platform-default policy are WASAPI IDs,
        // not ASIO CLSIDs. An explicit endpoint remains on its original backend.
        resolved.backend = "wasapi".into();
        if self.device_id.is_some() {
            return Ok(resolved);
        }
        let inventory = discover();
        if let Some(error) = inventory.discovery_error {
            return Err(error);
        }
        // Registry order has no default-device authority. Canonical identity
        // ordering is deterministic, and does not claim physical availability.
        if let Some(endpoint) = inventory.devices.into_iter().min_by(|a, b| a.id.cmp(&b.id)) {
            resolved.backend = "asio".into();
            resolved.device_id = Some(endpoint.id);
        }
        Ok(resolved)
    }

    pub fn cpal_device(&self) -> Result<(cpal::Host, cpal::Device), String> {
        let host = if self.backend == "auto" {
            cpal::default_host()
        } else {
            let id = cpal::available_hosts()
                .into_iter()
                .find(|id| id.name().to_ascii_lowercase() == self.backend)
                .ok_or_else(|| format!("Output backend '{}' is unavailable", self.backend))?;
            cpal::host_from_id(id).map_err(|error| error.to_string())?
        };
        let device = if let Some(selected) = &self.device_id {
            host.output_devices()
                .map_err(|error| error.to_string())?
                .find(|device| device.id().is_ok_and(|id| id.to_string() == *selected))
                .ok_or_else(|| "Selected output device is unavailable".to_owned())?
        } else {
            host.default_output_device()
                .ok_or_else(|| "No default output device found".to_owned())?
        };
        Ok((host, device))
    }
}

#[derive(Serialize)]
pub(super) struct OutputEndpoint {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
pub(super) struct OutputBackend {
    pub id: String,
    pub name: String,
    pub devices: Vec<OutputEndpoint>,
    pub discovery_error: Option<String>,
}

pub(super) fn cpal_inventory() -> Vec<OutputBackend> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| {
            let mut backend = OutputBackend {
                id: id.name().to_ascii_lowercase(),
                name: id.name().to_owned(),
                devices: Vec::new(),
                discovery_error: None,
            };
            match cpal::host_from_id(id)
                .map_err(|error| error.to_string())
                .and_then(|host| host.output_devices().map_err(|error| error.to_string()))
            {
                Ok(devices) => {
                    for device in devices {
                        match device
                            .id()
                            .map_err(|error| error.to_string())
                            .and_then(|id| {
                                device
                                    .description()
                                    .map(|description| OutputEndpoint {
                                        id: id.to_string(),
                                        name: description.name().to_owned(),
                                    })
                                    .map_err(|error| error.to_string())
                            }) {
                            Ok(endpoint) => backend.devices.push(endpoint),
                            Err(error) => backend.discovery_error = Some(error),
                        }
                    }
                }
                Err(error) => backend.discovery_error = Some(error),
            }
            backend
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_output_intent_rejects_coercion_and_partial_routing() {
        for json in [
            r#"{"buffer_frames":true}"#,
            r#"{"sample_rate_hz":0}"#,
            r#"{"backend":"ASIO"}"#,
            r#"{"left_channel":0}"#,
            r#"{"left_channel":2,"right_channel":2}"#,
            r#"{"unknown":0}"#,
        ] {
            assert!(OutputOptions::parse(Some(json)).is_err(), "{json}");
        }
        assert_eq!(OutputOptions::default().physical_pair(1).unwrap(), (0, 0));
        let options = OutputOptions::parse(Some(
            r#"{"backend":"asio","left_channel":4,"right_channel":5,"buffer_frames":96}"#,
        ))
        .unwrap();
        assert_eq!(options.physical_pair(12).unwrap(), (4, 5));
        assert!(options.physical_pair(4).is_err());
    }

    #[cfg(windows)]
    fn inventory(ids: &[&str], error: Option<&str>) -> OutputBackend {
        OutputBackend {
            id: "asio".into(),
            name: "ASIO".into(),
            devices: ids
                .iter()
                .map(|id| OutputEndpoint {
                    id: (*id).into(),
                    name: (*id).into(),
                })
                .collect(),
            discovery_error: error.map(str::to_owned),
        }
    }

    #[test]
    #[cfg(windows)]
    fn windows_auto_prefers_deterministic_asio_without_replacing_intent() {
        let requested = OutputOptions::parse(Some(
            r#"{"sample_rate_hz":96000,"buffer_frames":256,"left_channel":4,"right_channel":5}"#,
        ))
        .unwrap();
        let resolved = requested
            .resolve_windows_backend(|| inventory(&["{B}", "{A}"], None))
            .unwrap();
        assert_eq!(resolved.backend, "asio");
        assert_eq!(resolved.device_id.as_deref(), Some("{A}"));
        let mut expected = serde_json::to_value(&requested).unwrap();
        expected["backend"] = "asio".into();
        expected["device_id"] = "{A}".into();
        assert_eq!(serde_json::to_value(resolved).unwrap(), expected);
        assert_eq!(requested.backend, "auto");
        assert!(requested.device_id.is_none());
    }

    #[test]
    #[cfg(windows)]
    fn windows_auto_falls_back_only_after_successful_empty_discovery() {
        let requested = OutputOptions::default();
        let resolved = requested
            .resolve_windows_backend(|| inventory(&[], None))
            .unwrap();
        assert_eq!(resolved.backend, "wasapi");
        assert!(resolved.device_id.is_none());
        assert!(resolved.sample_rate_hz.is_none());
        assert!(resolved.buffer_frames.is_none());
        for ids in [vec![], vec!["{A}"]] {
            assert_eq!(
                requested
                    .resolve_windows_backend(|| inventory(&ids, Some("registry access denied")))
                    .unwrap_err(),
                "registry access denied"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn explicit_backends_and_saved_platform_ids_do_not_discover_or_change_identity() {
        for backend in ["asio", "wasapi", "unavailable"] {
            let requested = OutputOptions {
                backend: backend.into(),
                device_id: Some("chosen".into()),
                ..Default::default()
            };
            let resolved = requested
                .resolve_windows_backend(|| panic!("explicit backend must not discover"))
                .unwrap();
            assert_eq!(
                serde_json::to_value(resolved).unwrap(),
                serde_json::to_value(requested).unwrap()
            );
        }
        let requested = OutputOptions::parse(Some(r#"{"device_id":"WASAPI opaque endpoint","sample_rate_hz":44100,"buffer_frames":128,"left_channel":2,"right_channel":3}"#)).unwrap();
        let resolved = requested
            .resolve_windows_backend(|| panic!("saved platform endpoint must not become ASIO"))
            .unwrap();
        let mut expected = serde_json::to_value(&requested).unwrap();
        expected["backend"] = "wasapi".into();
        assert_eq!(serde_json::to_value(resolved).unwrap(), expected);
    }

    #[test]
    #[cfg(all(windows, feature = "asio-contract-tests"))]
    fn automatic_private_owner_uses_current_geometry_and_checked_close_without_fallback() {
        use crate::audio_engine::{AudioEngine, asio_output::test_driver::Factory};
        pyo3::Python::initialize();
        let factory = Factory::new().unwrap();
        let original_rate = factory.info().unwrap().rate;
        let mut engine = AudioEngine::new().unwrap();
        engine.test_asio_driver = Some(factory.clone());
        for geometry in [None, Some((96_000, 256))] {
            let mut options = OutputOptions::default();
            if let Some((rate, frames)) = geometry {
                options.sample_rate_hz = Some(rate);
                options.buffer_frames = Some(frames);
            }
            engine.start_audio_output(options.clone()).unwrap();
            let handle = engine.stream_handle.as_ref().unwrap();
            assert_eq!(handle.device_descriptor.host, "ASIO");
            let config = handle.configuration();
            assert_eq!(config["requested"], serde_json::to_value(options).unwrap());
            assert_eq!(config["device_id"], "private-asio-fixture");
            let observer = handle.test_owner_observer();
            let driver = observer.lock().unwrap().driver;
            assert_eq!(
                handle.output_sample_rate,
                geometry.map_or(original_rate as u32, |(rate, _)| rate)
            );
            assert_eq!(
                config["initialized_buffer_frames"],
                geometry.map_or(driver.preferred_frames as u32, |(_, frames)| frames)
            );
            handle.test_tick().unwrap();
            engine.shut_down().unwrap();
            let ended = observer.lock().unwrap();
            assert!(
                ended.owner_ended
                    && ended.owner_join_verified
                    && ended.retirement_verified
                    && ended.backend_released
            );
            assert_eq!(
                factory.info().unwrap().rate,
                original_rate,
                "unfinished candidate restores its original driver rate"
            );
        }
        // The selected private driver opens, but refuses this illegal buffer.
        // The original error and checked pending-owner close survive; no CPAL open.
        let options = OutputOptions {
            buffer_frames: Some(95),
            ..Default::default()
        };
        let error = engine.start_audio_output(options).unwrap_err().to_string();
        assert!(error.contains("ASIO"), "{error}");
        assert!(engine.stream_handle.is_none() && engine.pending_output.is_none());
        engine.shut_down().unwrap();
        let info = factory.info().unwrap();
        assert_eq!(info.active, 0);
        assert_eq!(info.created, info.released);
        assert_eq!(info.created, 3);
    }
}
