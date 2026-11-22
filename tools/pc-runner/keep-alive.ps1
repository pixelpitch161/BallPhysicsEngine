# Keeps the bpe-ci WSL machine (the checks' runners) running while the owner is logged in. Started at logon by the
# "BPE checks (WSL)" scheduled task; see docs/notes/checks.md. (Windows won't let even an elevated task change the
# VM process's priority, so the runners' low priority is set inside the machine, in bpe-runner@.service.)
Start-Process -FilePath wsl.exe -ArgumentList '-d', 'bpe-ci', '--exec', '/bin/sleep', 'infinity' -WindowStyle Hidden
