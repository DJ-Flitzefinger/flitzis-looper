class AudioMessage:
    """Message that is emitted from the audio thread."""

    class Ping(AudioMessage):
        def __init__(self) -> None: ...

    class Pong(AudioMessage):
        """Response to a Ping message."""
        def __init__(self) -> None: ...

    class Stopped(AudioMessage):
        """Indicates the audio playback is stopped."""
        def __init__(self) -> None: ...

class AudioEngine:
    """AudioEngine provides minimal audio output capabilities using cpal."""

    def __init__(self) -> None:
        """Create a new AudioEngine instance with default audio device."""
        ...

    def run(self) -> None:
        """Initialize and run the audio engine."""
        ...

    def shut_down(self) -> None:
        """Shut down the audio engine."""
        ...

    def ping(self) -> None:
        """Send a ping message to the audio thread."""
        ...

    def receive_msg(self) -> AudioMessage | None:
        """Receive a message from the audio thread."""
        ...
