# Local tiny-skia integration patch

Source: crates.io `iced_tiny_skia` 0.14.1, Iced commit
`0ecf60664df7b8ac7d7aef5f7279d5323027f693`, directory `tiny_skia`.
Upstream: https://github.com/iced-rs/iced

In `src/window/compositor.rs`, after upstream damage grouping,
more than eight repaint regions are combined into their bounding rectangle.
This bounds repeated scene replay and window-sized clipping-mask work during
dense Process List updates. Sparse updates retain upstream partial redraws.
The regression test compares a coalesced repaint with a full repaint, including
translucent content. No timing instrumentation is included.

`src/primitive.rs` includes stroke width, caps, joins, and antialiasing in repaint
bounds. Upstream path-only bounds give horizontal and vertical strokes zero area,
so flat chart lines and hover guides can be omitted from partial redraws.
Remove this correction when upstream stroke bounds cover the painted area.

Run the regression with `cargo test --locked -p iced_tiny_skia --lib`.
Remove this patch when an upstream release resolves the fragmented repaint cost
and passes the same live scrolling and memory checks.
