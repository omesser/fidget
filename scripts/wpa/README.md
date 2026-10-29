# WPA export profiles

`ExportCPUUsageSampled.wpaProfile` is from Bruce Dawson's
[UIforETW](https://github.com/google/UIforETW/blob/main/bin/ExportCPUUsageSampled.wpaProfile),
Apache License 2.0, unchanged apart from line endings: CRLF became LF and
the file gained a final newline. It has `wpaexporter.exe` write the CPU Usage
(Sampled) table as `CPU_Usage_(Sampled)_Randomascii_Export.csv`, one row per
sample, with the columns Process, Thread ID and Stack, leaving out DPC/ISR
time and the Idle process.

`scripts/export-mask-flame-windows.ps1` runs it on a trace from
`scripts/bench-mask-rebuild-windows-flame.ps1` and folds the rows into a
flamegraph (#428).
