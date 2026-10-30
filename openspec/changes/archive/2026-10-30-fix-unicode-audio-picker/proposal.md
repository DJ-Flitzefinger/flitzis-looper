# Preserve Unicode audio selections

The current ImFileDialog binding converts Windows filesystem paths to narrow
strings. Selecting `Veteran’s Memorial.flac` raises UTF-8 decoding errors before
load admission and escapes into the render loop.

Use the already installed wide-character Portable File Dialogs backend in one
owned helper process. Retain nonblocking UI polling, exact Unicode paths and
captured PAD ownership. Isolating the native dialog avoids its in-process
startup/cancel future race. Cancellation, picker failure and closing preserve
existing source assignments, settings and controller settlement.
