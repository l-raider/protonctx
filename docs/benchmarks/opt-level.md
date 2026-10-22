# `publicrelease` opt-level benchmark

Measured 2026-10-22 on protonctx 1.5.0 (`2f670f7` + `b1e7af1`), Slint 1.18.1,
rustc 1.98.1. Result: **opt-level 2**. It had the lowest CPU under
interaction; idle CPU is 0 at every level. The priority was low CPU, then low
memory, then small binary.

## Setup

- `publicrelease` profile (fat LTO, `codegen-units = 1`, `panic = "abort"`,
  stripped), with only `opt-level` overridden:
  `cargo build --profile publicrelease --config 'profile.publicrelease.opt-level=<n>'`.
- `.cargo/config.toml` rustflags from `b1e7af1`: lld, `--icf=all`,
  `target-cpu=x86-64-v3`.
- Each binary run under Xvfb (1000×700×24, no window manager,
  `WAYLAND_DISPLAY` unset) with the live KDE theme and the same six games.
- Three rounds, with the order rotated per round (1 2 3 s z / z s 3 2 1 /
  3 1 z 2 s).
- Per run ([`probe.sh`](probe.sh)):
  - start-up CPU over the first 4 s
  - PSS/RSS from `/proc/<pid>/smaps_rollup`
  - idle CPU over the next 30 s
  - CPU during a 10 s pointer sweep over the table rows at 120 Hz, via XTest
    ([`hover.py`](hover.py))
- CPU comes from utime+stime in 10 ms ticks (`CLK_TCK` = 100), so
  differences of one or two ticks are noise.

## Results (mean of three runs)

| opt-level | Binary | `xz -9` | PSS idle | RSS idle | Idle CPU (30 s) | Start-up CPU | 10 s hover sweep CPU | Clean build |
|---|---|---|---|---|---|---|---|---|
| 1 | 18.7 MB | 6.0 MB | 19.7 MB | 27.7 MB | 0 ms | 40 ms | 123 ms (100–150) | 110 s |
| **2** | 18.0 MB | 6.1 MB | 18.6 MB | 26.6 MB | 0 ms | 33 ms | **77 ms** (70–80) | 171 s |
| 3 | 18.3 MB | 6.2 MB | 18.6 MB | 26.6 MB | 0 ms | 30 ms | 93 ms (80–110) | 179 s |
| s | 14.8 MB | 5.3 MB | 16.6 MB | 24.6 MB | 0 ms | 37 ms | 113 ms (90–130) | 133 s |
| z | 13.8 MB | 5.1 MB | 15.7 MB | 23.7 MB | 0 ms | 40 ms | 140 ms (120–160) | 117 s |

Binary sizes in bytes: O1 18 732 128, O2 18 035 720, O3 18 298 248,
Os 14 778 488, Oz 13 835 896.

## Reading

- **Idle:** no level uses any CPU when nothing changes; start-up is within
  noise (30–40 ms).
- **Interaction:** this is the only CPU difference.
  - Level 2 was lowest and the most stable: about 0.8% of one core during
    constant pointer motion.
  - Level 3 was not faster and is larger.
  - `z` was the slowest, at about 1.4% of a core.
- **Memory and size** fall from 1/2/3 to `s` to `z`. Level 2 costs about 2 MB
  PSS and 3.2 MB of binary (0.8 MB compressed) more than `s`.
- If only literal idle mattered, CPU would tie and memory would pick `z`.

## Raw data

Columns: label, start-up CPU (ms), idle CPU over 30 s (ms), hover-sweep CPU
(ms), PSS idle (KB), RSS idle (KB), PSS after the hover sweep (KB).

```
O1 60 0 100 19515 27668 19499
O2 40 0 70 18598 26768 18479
O3 30 0 90 18514 26684 18379
Os 40 0 90 16591 24708 16592
Oz 40 0 140 15551 23708 15375
Oz 40 0 160 15613 23808 15613
Os 40 0 130 16423 24488 16422
O3 30 0 110 18271 26312 18280
O2 30 0 80 18371 26388 18558
O1 30 0 120 19812 27700 19812
O3 30 0 80 18914 26832 18915
O1 30 0 150 19709 27740 19709
Oz 40 0 120 15851 23708 15852
O2 30 0 80 18683 26560 18683
Os 30 0 120 16682 24696 16682
```

## Re-running

```sh
for o in 1 2 3 s z; do
  v=$o; [[ $o == [sz] ]] && v="\"$o\""
  cargo build --profile publicrelease --config "profile.publicrelease.opt-level=$v" \
    --target-dir target/opt-$o
done
xvfb-run -a -s "-screen 0 1000x700x24" docs/benchmarks/probe.sh O2 \
  target/opt-2/publicrelease/protonctx
```

The probe needs Xvfb (`xvfb-run`), `python3` and `libXtst`. The main window must be at 0,0, which it is under Xvfb without a
window manager.
