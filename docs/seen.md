# seen

`seen` reads image bytes from a Unix socket and broadcasts a one-sentence caption.

## Input Protocol

Image captions use the time the first frame is received. Streams no longer need a leading `@{timestamp}` line.
