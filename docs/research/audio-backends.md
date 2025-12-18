# Cross-Platform Audio Backends and I/O

Achieving sub-10ms latency on Windows, macOS, and Linux typically means using the native low‑latency APIs (ASIO or WASAPI on Windows, CoreAudio on macOS, ALSA/Jack on Linux). Common C/C++ I/O libraries wrap these: e.g. [**PortAudio**][portaudio] (C) or [**RtAudio**][rtaudio] (C++) provide a uniform API for real‑time streams. In Rust, the [**CPAL**][cpal] crate offers similar cross‑platform device support (ALSA/JACK on Linux, WASAPI/ASIO on Windows, CoreAudio on macOS). For flexibility, **JACK** can be used on all platforms (providing pro‑audio routing and low latency) while default backends (WASAPI/ASIO, CoreAudio, ALSA) are generally recommended for ease of use.

If using C/C++, libraries like [**miniaudio**][miniaudio] (single-file C/C++) are very attractive: it's cross‑platform (Windows, macOS, Linux, mobile, Web) with no external deps, and it includes built‑in device management for all major APIs. **miniaudio** supports a callback model or a high‑level "engine" with mixing nodes and effects graphs. Porting **miniaudio** to Rust is also possible via FFI (e.g. the *miniaudio-sys* crate). Other engines like **JUCE** (C++) or **LabSound** (C++, MIT-licensed) provide rich audio/DSP frameworks, but JUCE's licensing (GPL/commercial) and size can be heavy for a two‑dev team. A lightweight alternative is [**SoLoud**][soloud] (C++), a game-oriented engine with sample playback, filters, mixer buses and gapless looping. SoLoud even has Python bindings, though it's noted to lack hand‑optimized SIMD, so its raw speed may lag highly tuned libraries.

## Mixing, Looping and Effects

Most of these engines handle multi‑voice mixing and FX. For example, **SoLoud** supports multiple concurrent *voices*, adjustable playback speed/pitch, filters (low/high‑pass, echo, etc.), mixer busses, and *gapless looping*. Likewise **miniaudio** exposes a high‑level node graph: you create sound nodes (e.g. decoded audio samples) and connect them through effect nodes. Its "node graph" API lets you route sounds into filters, mixers and buses in any topology. In practice you'd load each loop into a sound source node (buffer or streaming), and then adjust its volume/EQ via connected filter nodes in real time. For EQ or dynamics, you can use built‑in biquad filter nodes (miniaudio) or write your own DSP. Engine libraries often handle resource management too (streaming large files, ref‑counting samples).

If using a minimal I/O library (e.g. PortAudio or CPAL), you'd implement mixing manually in the audio callback: maintain 20+ ring buffers for loops, apply any FX, and sum to the output. This is more work but maximally efficient. For many developers, using a ready engine like miniaudio or SoLoud avoids "reinventing the wheel". Given the live‑performance goal and a small team, a modular library (miniaudio or SoLoud) plus specialized DSP modules is advisable.

## Time-Stretching and Pitch Shifting

Achieving *perfect loops* often implies independent control of pitch and time. There are dedicated libraries for this. [**Rubber Band**][rubberband] (C++, GPL) is a high‑quality time‑stretch/pitch-shift library that lets you change tempo and pitch independently. It's proven in audio apps, but its GPL license may be restrictive for closed-source products (commercial licensing is offered). [**SoundTouch**][soundtouch] (C/C++, LGPL) is another well-known library: it can change tempo (time-stretch) without altering pitch, or change pitch without altering tempo. It's older and can incur ~100ms latency in worst case, but it's lightweight and easy to integrate. A more modern MIT‑licensed solution is [**Signalsmith Stretch**][signalsmith] (C++/Rust): a high-quality pitch/time algorithm described in an Audio Developers Conference talk. Rust bindings ([ssstretch][ssstretch] crate) or ports ([signalsmith-dsp][signalsmith-dsp]) exist.

In practice, you'd decode loop buffers (e.g. WAV/OGG) and feed them through a time‑stretch module when playing. If sub-10ms re-scheduling is critical, you might preload loop data in memory and use a small overlap-add buffer. Many audio engines don't include time-stretch by default, so picking one of the above libraries (calling from C++ or via FFI in Rust) is key if adjustable pitch is needed. For simple pitch shift (with time-coupled change), you could also vary playback rate (resample), but this isn't a real time-stretch.

## Rust Ecosystem

Rust's audio ecosystem is maturing but smaller than C/C++. For I/O, [**CPAL**][cpal] (pure Rust) wraps the native backends. On top of CPAL, [**rodio**][rodio] offers a simple playback API (suitable for game audio) and can do mixing and output multiple sounds, but it's not designed for very low-latency pro audio. The [**kira**][kira] crate is a higher-level audio library for games in Rust (mixer, scheduling) and even has a Python binding (via `pyo3`). For DSP, the [**dasp**][dasp] family of crates provides filters, FFTs, etc. There's also [**kittyaudio**][kittyaudio], a Rust playback library emphasizing low latency and loops. It handles playback and basic commands (volume, rate, pan, loops), but note its roadmap still lacks effects. If you prefer Rust but need proven DSP, you can still call C/C++ libraries (via FFI) for time-stretch or other heavy tasks.

In summary, a Rust stack might use CPAL for audio I/O, a Rust engine like kittyaudio (or your own mixer loop) for playback, and crates or FFI for DSP.

## Integrating with Python (GUI)

Because the audio engine needs real-time throughput, it should run outside Python's GIL. The typical approach is to implement the audio callback and DSP in native code (C/C++/Rust), so that Python threads don't block it. One can embed this as a Python extension or run it as a separate process. A useful pattern is: the audio callback (in C/Rust) simply reads/writes from a [lock-free ring buffer][lock-free-ring-buffer], while Python threads communicate with it asynchronously. For example, the callback would have `nogil` in Cython or no `lock()` in C code, and the Python side pushes loop buffers or control messages into the buffer. If absolute lowest latency is needed, a separate process can isolate the engine: IPC (shared memory or real-time messaging) passes control data, while the audio process drives the sound card. 3rd-party routing (e.g. using JACK as a sound server) is another option to link a standalone engine and a GUI.

In practice, even if the front-end GUI is Python, the audio graph should run on its own thread or core. Python's GIL means any Python code in the audio path adds jitter, so audio libraries often release the GIL in callbacks. Designing a ring-buffer or socket protocol (with minimal copying) ensures that the Python UI can enqueue events (start/stop loops, change FX) without disturbing real-time audio.

## Real-Time Performance Tips

To hit <10ms latency, use the lowest-level APIs possible and small buffers. For example, on Windows use **WASAPI in exclusive mode** or ASIO, on Linux use ALSA or JACK (PulseAudio/JACK hybrid can give ~10ms). Testing is key: on many systems you may need a few milliseconds buffer (e.g. 128 samples ~3ms at 44.1kHz). Always avoid memory allocation or locks in the audio callback. Preallocate audio buffers, decode files ahead of time, and use CPU-friendly code (SIMD, no GC). Ensure the audio thread runs at high priority (real-time scheduling if permitted). As a rule, the audio thread should do *only* mixing and DSP; any file loading, UI work or heavy computation belongs in a non-realtime thread.

Finally, for FX chains/EQ: use simple IIR/Biquads or the engine's built‑in filters. If you want more complex effects (reverb, delay), consider writing them as additional node graph stages or use existing DSP code. Some teams use plugin APIs (VST/LV2) but that adds complexity; a 2‑dev team might code just the needed filters.

## Summary

A practical approach is to pick a robust cross-platform audio library (like miniaudio or SoLoud in C/C++, or CPAL-based code in Rust), augment it with a time-stretch library (RubberBand, SoundTouch, or Signalsmith) for pitch/tempo, and run the engine in its own native thread/process with ring‑buffer IPC from Python. This covers all target OSes with low latency, supports many loops with effects, and avoids "reinventing the wheel" by reusing battle‑tested libraries.

[portaudio]: https://www.portaudio.com/
[rtaudio]: https://github.com/thestk/rtaudio
[cpal]: https://github.com/RustAudio/cpal
[rodio]: https://github.com/RustAudio/rodio
[kira]: https://github.com/tesselode/kira
[dasp]: https://github.com/RustAudio/dasp
[miniaudio]: https://miniaud.io/
[soloud]: https://solhsa.com/soloud/
[rubberband]: https://breakfastquay.com/rubberband/
[soundtouch]: https://www.surina.net/soundtouch/
[signalsmith]: https://signalsmith-audio.co.uk/code/stretch/
[kittyaudio]: https://github.com/zeozeozeo/kittyaudio
[lock-free-ring-buffer]: https://github.com/spatialaudio/python-rtmixer/issues/1
[ssstretch]: https://github.com/bmisiak/ssstretch
[signalsmith-dsp]: https://github.com/CuteDSP/RSCuteDSP
