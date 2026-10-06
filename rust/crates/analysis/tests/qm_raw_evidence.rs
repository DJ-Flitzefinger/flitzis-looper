//! Public QM capture contracts, without count inference or signal acceptance.

use flitzis_looper_analysis::{
    AnalysisConfig, DetectionFunction, DownBeat, TempoTrackV2, analyze_bpm, analyze_bpm_raw,
    calculate_bpm,
};

#[test]
fn raw_capture_retains_complete_tracker_output_timebase_and_configuration() {
    for sample_rate_hz in [44_100, 48_000, 96_000] {
        let mut config = AnalysisConfig {
            step_secs: 0.01031,
            max_bin_hz: 60.0,
            input_tempo: 123.75,
            alpha: 0.87,
            tightness: 3.9,
            // These three fields are recorded, not newly applied by this capture.
            viterbi_sigma: 5.25,
            window_length: 321,
            hop_size: 65,
        };
        let saved_config = config.clone();
        let hop = (sample_rate_hz as f64 * config.step_secs) as usize;
        let period_samples = 43 * hop;
        let offset_samples = 3 * hop;
        let mut audio = vec![0.0; offset_samples + 32 * period_samples + hop / 2 + 11];
        for beat in 0..32 {
            let start = offset_samples + beat * period_samples;
            for offset in 0..64 {
                audio[start + offset] = 1.0 - offset as f64 / 64.0;
            }
        }
        let input_frame_count = audio.len();

        // Independently invoke the public detector/tracker to obtain its complete
        // pre-publication output; a raw-versus-legacy equality alone could hide
        // a shared crop, renumbering or coordinate conversion regression.
        let mut detection = DetectionFunction::new(sample_rate_hz, &config);
        assert_eq!(detection.step_size_samples(), hop);
        assert_ne!(input_frame_count % hop, 0);
        let odf = detection.process(&audio);
        let frame_duration = hop as f64 / sample_rate_hz as f64;
        assert_ne!(frame_duration.to_bits(), config.step_secs.to_bits());
        let tracker = TempoTrackV2::new(sample_rate_hz as f64, frame_duration);
        let mut periods = Vec::new();
        tracker.calculate_beat_period(&odf, &mut periods, config.input_tempo, false);
        let mut expected_frames = Vec::new();
        tracker.calculate_beats(
            &odf,
            &periods,
            &mut expected_frames,
            config.alpha,
            config.tightness,
        );
        let mut expected_downbeats = Vec::new();
        DownBeat::new(sample_rate_hz as f64, 16, hop).find_downbeats(
            &audio,
            input_frame_count,
            &expected_frames,
            &mut expected_downbeats,
        );
        assert!(expected_frames.len() >= 28);
        assert!(expected_frames[0] * frame_duration < 0.5);
        assert!(expected_frames.last().unwrap() * frame_duration > 13.0);
        assert!(!expected_downbeats.is_empty());

        let raw = analyze_bpm_raw(&audio, sample_rate_hz, &config).unwrap();
        let (legacy_bpm, legacy_grid) = analyze_bpm(&audio, sample_rate_hz, &config).unwrap();
        // Capture ownership is independent of subsequent input/config mutations.
        audio.fill(0.0);
        config.step_secs = 0.5;
        config.max_bin_hz = 20.0;
        config.input_tempo = 60.0;
        config.alpha = 0.1;
        config.tightness = 1.0;
        config.viterbi_sigma = 1.0;
        config.window_length = 1;
        config.hop_size = 1;
        drop(audio);

        assert_eq!(raw.input_sample_rate_hz(), sample_rate_hz);
        assert_eq!(raw.input_frame_count(), input_frame_count);
        assert_eq!(raw.odf_hop_samples(), hop);
        assert_eq!(raw.configuration(), &saved_config);
        assert_ne!(raw.configuration(), &config);
        assert_eq!(raw.beat_frames(), expected_frames);
        assert_eq!(raw.downbeat_raw_indices(), expected_downbeats);
        assert_eq!(raw.beat_seconds().len(), expected_frames.len());
        assert_eq!(raw.downbeat_seconds().len(), expected_downbeats.len());

        let expected_seconds: Vec<f64> = expected_frames
            .iter()
            .map(|frame| *frame * frame_duration)
            .collect();
        let seconds: Vec<f64> = raw.beat_seconds().collect();
        for (actual, expected) in seconds.iter().zip(&expected_seconds) {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        let expected_downbeat_seconds: Vec<f64> = expected_downbeats
            .iter()
            .map(|index| expected_seconds[*index])
            .collect();
        assert_eq!(
            raw.downbeat_seconds().collect::<Vec<_>>(),
            expected_downbeat_seconds
        );

        let expected_bpm = calculate_bpm(&expected_frames, frame_duration);
        let expected_legacy_beats: Vec<f32> = expected_seconds
            .iter()
            .map(|seconds| *seconds as f32)
            .collect();
        let expected_legacy_downbeats: Vec<f32> = expected_downbeats
            .iter()
            .map(|index| expected_legacy_beats[*index])
            .collect();
        let (projected_bpm, projected_grid) = raw.legacy_result();
        assert_eq!(projected_bpm.to_bits(), expected_bpm.to_bits());
        assert_eq!(legacy_bpm.to_bits(), expected_bpm.to_bits());
        for grid in [projected_grid, legacy_grid] {
            assert_eq!(grid.beats, expected_legacy_beats);
            assert_eq!(grid.downbeats, expected_legacy_downbeats);
            assert_eq!(grid.bars, expected_legacy_downbeats);
        }
    }
}
