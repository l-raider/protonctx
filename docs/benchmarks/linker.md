# Linker benchmark: lld vs mold

Measured 2026-10-22 on protonctx 1.5.0 (`6d3ab97`), rustc 1.98.1, LLD 22.1.8,
mold 2.40.4, on 8 cores. Every config used `-C target-cpu=x86-64-v3`, passed
through `RUSTFLAGS`, which overrides `.cargo/config.toml`.

Result: the differences are too small to matter, so **lld with
`--icf=all` stays**, because it is the more widely available linker.
mold with `--icf=all` produces the same release binary, and only its
incremental dev rebuilds are faster, by about 0.25 s (13%).

## Setup

- Configs:
  - `lld-icf`: `-fuse-ld=lld -Wl,--icf=all`, the config as of `b1e7af1`
  - `lld`: `-fuse-ld=lld`
  - `mold`: `-fuse-ld=mold`
  - `mold-icf`: `-fuse-ld=mold -Wl,--icf=all`
- Each config built in its own target directory:
  - a clean dev build
  - 5 incremental dev rebuilds, each after appending a comment line to
    `src/app.rs`
  - a clean `publicrelease` build (opt-level 2, fat LTO)
  - one incremental `publicrelease` rebuild
- The dev measurements ran twice, in opposite order
  (`lld-icf lld mold mold-icf`, then reversed). The first config of the first
  pass ran from a cold disk cache (52 s clean), so the dev clean time uses the
  second pass.
- Runtime: the release binaries went through [`probe.sh`](probe.sh) under
  Xvfb, two rounds each (see [opt-level.md](opt-level.md) for the method).

## Results

| Config | Incremental dev rebuild (median of 10) | Clean dev build | Clean release build | Incremental release | Release binary | PSS idle | Idle CPU (30 s) | 10 s hover sweep CPU |
|---|---|---|---|---|---|---|---|---|
| lld-icf | 1.93 s | 43.4 s | 177 s* | 96.8 s | 18 035 720 B | 18.6 MB | 0 ms | 75 ms |
| lld | 1.78 s | 42.9 s | 160 s | 96.5 s | 18 289 800 B | 18.6 MB | 0 ms | 85 ms |
| mold | **1.65 s** | 42.9 s | 165 s | 96.2 s | 18 287 552 B | 18.7 MB | 0 ms | 75 ms |
| **mold-icf** | 1.68 s | 43.2 s | 161 s | 95.9 s | **18 033 472 B** | 18.5 MB | 0–10 ms | 75 ms |

\* The clean release builds are a single sample each, and `lld-icf` ran
first; expect about ±10 s of noise.

## Reading

- **Incremental dev rebuilds**, the edit-compile loop and the only place the
  linker clearly shows: mold is 0.13–0.25 s faster than lld.
  - `--icf=all` costs lld about 150 ms but mold only about 30 ms.
  - The rest of the ~1.7 s is rustc compiling protonctx itself, which is the
    same for every config.
- **Clean builds:** within noise. Compiling the dependencies dominates.
- **Release builds:** fat LTO dominates (~96 s incremental), so the linker
  makes no difference.
- **Binary size:** `--icf=all` saves about 254 KB (1.4%) with either linker.
  mold and lld produce sizes within 2.3 KB of each other.
- **Runtime:** PSS, idle CPU and hover CPU are the same within noise; ICF
  folds identical functions and has no measurable runtime cost.
- **Dev binaries** (debug info) differ in size: lld-icf 354 MB, lld 359 MB,
  mold-icf 363 MB, mold 381 MB. Only disk use is affected.

## Raw data

Dev builds, ms (pass 1, then pass 2):

```
lld-icf  clean 51955 / 43396   incr 2099 1952 1935 1949 1928 / 2259 1892 1886 1878 1878
lld      clean 44458 / 42938   incr 1809 1772 1805 1762 1772 / 1786 1803 1775 1754 1833
mold     clean 44359 / 42908   incr 1791 1794 1724 1744 1687 / 1577 1614 1593 1570 1579
mold-icf clean 43590 / 43220   incr 1661 1683 1655 1699 1672 / 1699 1677 1710 1649 1674
```

Release builds, ms: clean / incremental:

```
lld-icf 177283 / 96820    lld 160446 / 96530    mold 164865 / 96246    mold-icf 160550 / 95926
```

Probe (label, start-up CPU ms, idle 30 s CPU ms, hover CPU ms, PSS idle KB,
RSS idle KB, PSS after hover KB):

```
lld-icf 30 0 80 18437 26528 18437
lld 30 0 80 18447 26540 18447
mold 40 0 70 18769 26700 18769
mold-icf 30 0 80 18397 26428 18397
lld-icf 30 0 70 18678 26760 18678
lld 20 0 90 18830 26780 18830
mold 30 0 80 18673 26616 18673
mold-icf 20 10 70 18635 26728 18635
```
