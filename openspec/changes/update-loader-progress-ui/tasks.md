## 1. Implementation
- [ ] 1.1 Add stage support to loader progress events (Rust `LoaderEvent::Progress` + Python dict fields)
- [ ] 1.2 Implement a total-progress reporter across sub-tasks with sensible weights and throttling
- [ ] 1.3 Emit progress updates from the decode/load loop and from resampling (or at least at sub-task boundaries)
- [ ] 1.4 Extend Python session state to track per-pad load stage (alongside existing progress)
- [ ] 1.5 Update the pad grid rendering to show `stage + percent` and draw an in-pad progress bar background
- [ ] 1.6 Update the selected-pad sidebar to show `stage + percent` while loading

## 2. Validation
- [ ] 2.1 Run Rust tests (`cargo test`) for loader/resampler changes
- [ ] 2.2 Run Python tests (`pytest`) to ensure controller/UI state handling remains correct
- [ ] 2.3 Manual smoke: load a file and observe multiple progress updates + pad progress bar

## 3. Follow-ups (Optional)
- [ ] 3.1 Tune progress weights based on observed UX