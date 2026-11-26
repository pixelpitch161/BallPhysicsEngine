# The all-drops engine's part C2 (deep water and landings) runs on the owner's graphics card
# (docs/superpowers/plans/water-mode/core-c2-deep-water.md, Task 10). C1's shots, tables and scene
# (card/all-drops-core-c1/run.ps1 on C1's branch), so the films line up with C1's and the trial's
# side by side, and C2's own: the pools for 20 s at 6, 3 and 2 px, the landings, the shelves.
# Removed from the branch before the merge. Run natively on Windows, from a folder outside the
# project:
#
#   powershell -ExecutionPolicy Bypass -File run.ps1 -Exe <shots test exe> -Kit <water_core test exe>
#       -Out <results folder> -Scene "<Water Weird.ron>" [-Only films,style,...]
#
# Every run's printed table lands in <Out>\<name>.txt, stills as PNGs in <Out>, film frames in
# <Out>\<shot>-c<cell>[-tag]\ and are joined into <Out>\films\<same name>.mp4 (frames deleted).
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Kit,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Scene = "",
    [string[]]$Only = @()
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $Out, "$Out\films" | Out-Null
$names = "WATER_OUT", "WATER_SHOTS", "WATER_CELLS", "WATER_SECONDS", "WATER_FPS", "WATER_STILLS", "WATER_TAG",
    "WATER_FILM", "WATER_FILM_SCALE", "WATER_DOTS", "WATER_PULL", "WATER_STICK", "WATER_SHIFT", "WATER_FROM",
    "WATER_REST", "WATER_SLOSH", "WATER_SCENE", "WATER_DETAILS", "WATER_CORES", "WATER_SIZE", "WATER_MEASURE",
    "BPE_WATER_ITERS", "BPE_WATER_JOIN", "BPE_WATER_DEEP"

function Wanted([string]$group) { return ($Only.Count -eq 0) -or ($Only -contains $group) }

# One run of a test exe: `$set` are its environment variables, `$test` the test's name.
function Run([string]$log, [string]$program, [string]$test, [hashtable]$set) {
    foreach ($n in $names) { Remove-Item "Env:$n" -ErrorAction SilentlyContinue }
    $env:WATER_OUT = $Out
    foreach ($k in $set.Keys) { Set-Item "Env:$k" ([string]$set[$k]) }
    $began = Get-Date
    & $program --ignored --nocapture --test-threads=1 $test 2>&1 | Out-File -Encoding utf8 -Append "$Out\$log.txt"
    "# ${test} took $([int]((Get-Date) - $began).TotalSeconds) s, exit ${LASTEXITCODE}: $($set | ConvertTo-Json -Compress)" |
        Out-File -Encoding utf8 -Append "$Out\$log.txt"
    Films
}

# Joins every folder of frames into a film and deletes the frames.
function Films {
    Get-ChildItem -Directory $Out | Where-Object { Test-Path "$($_.FullName)\f00000.png" } | ForEach-Object {
        $film = "$Out\films\$($_.Name).mp4"
        & ffmpeg -y -loglevel error -framerate 30 -i "$($_.FullName)\f%05d.png" -c:v libx264 -crf 20 -preset slow `
            -pix_fmt yuv420p -vf "pad=ceil(iw/2)*2:ceil(ih/2)*2" $film
        if (Test-Path $film) { Remove-Item -Recurse -Force $_.FullName }
    }
}

# The trial's feel: Surface pull 0.5 (the new default), Stickiness 0.5.
$feel = @{ WATER_PULL = "0.5"; WATER_STICK = "0.5" }
$seconds = @{ slope = 5; ledge = 5; pool = 6; course = 8; beads = 4; weird = 6 }

# 1. The six shots at Fine (6 px) and Extreme (3 px), in the plain drawing.
if (Wanted "films") {
    foreach ($cell in "6", "3") {
        foreach ($shot in "slope", "ledge", "pool", "course", "beads", "weird") {
            $s = $seconds[$shot]
            Run "films" $Exe "shots" ($feel + @{ WATER_SHOTS = $shot; WATER_CELLS = $cell; WATER_SECONDS = $s
                    WATER_FPS = 30; WATER_STILLS = "1,2,$($s - 1)"; WATER_FILM_SCALE = 1; WATER_DOTS = 0 })
        }
    }
}

# 2. Surface pull low and high, at Fine and Extreme.
if (Wanted "style") {
    foreach ($pull in "0.2", "1") {
        foreach ($cell in "6", "3") {
            Run "style" $Exe "shots" @{ WATER_SHOTS = "slope,ledge,beads"; WATER_CELLS = $cell; WATER_SECONDS = 4
                WATER_FPS = 30; WATER_STILLS = "2,3"; WATER_FILM_SCALE = 1; WATER_DOTS = 0; WATER_PULL = $pull
                WATER_STICK = "0.5"; WATER_TAG = "pull$pull"
            }
        }
    }
}

# 3. At 2 pixel cells.
if (Wanted "two") {
    Run "two" $Exe "shots" ($feel + @{ WATER_SHOTS = "slope,weird,pool"; WATER_CELLS = "2"; WATER_SECONDS = 4
            WATER_FPS = 30; WATER_STILLS = "2,3"; WATER_FILM_SCALE = 1; WATER_DOTS = 0 })
}

# 4. Speed: wall seconds for a scene second, physics alone, nothing read back in between; and the
# same without a step's end joined to the next step's fall.
if (Wanted "pace") {
    Run "pace" $Exe "pace" ($feel + @{ WATER_SHOTS = "weird,slope,ledge,pool,course,deep"; WATER_CELLS = "8,6,3"
            WATER_FROM = 2; WATER_SECONDS = 1 })
    Run "pace" $Exe "pace" ($feel + @{ WATER_SHOTS = "weird,slope,pool,deep,fill"; WATER_CELLS = "2"; WATER_FROM = 2
            WATER_SECONDS = 1 })
    # The same without the deep part, for what it costs.
    Run "pace" $Exe "pace" ($feel + @{ WATER_SHOTS = "weird,pool,course,deep"; WATER_CELLS = "6,3"
            WATER_FROM = 2; WATER_SECONDS = 1; BPE_WATER_DEEP = 0 })
}

# 5. The deep pool at rest and its slosh.
if (Wanted "pools") {
    Run "pools" $Exe "pools" @{ WATER_CELLS = "6,3"; WATER_REST = 20 }
    Run "pools" $Exe "pools" @{ WATER_CELLS = "2"; WATER_REST = 20 }
}

# 5b. A hard landing's pieces (Water Weird's shot) at Surface pull 0.2 and 0.5.
if (Wanted "landing") {
    Run "landing" $Exe "landing" @{ WATER_CELLS = "8,6,3,2"; WATER_SECONDS = 3 }
}

# 5c. The course's top shelf as it drains, at Stickiness 0 and 0.5.
if (Wanted "shelves") {
    foreach ($stick in "0", "0.5") {
        Run "shelves" $Exe "shelves" @{ WATER_CELLS = "6,3"; WATER_SECONDS = 30; WATER_STICK = $stick }
    }
}

# 6. The course: where the water is after 30 seconds.
if (Wanted "openings") {
    Run "openings" $Exe "openings" ($feel + @{ WATER_CELLS = "6,3"; WATER_SECONDS = 30 })
}

# 7. Does a run repeat: within a run of the program, and between two runs of it.
if (Wanted "repeats") {
    Run "repeats" $Exe "repeats" ($feel + @{ WATER_SHOTS = "weird,pool"; WATER_CELLS = "6,3"; WATER_SECONDS = 2 })
    Run "repeats" $Exe "repeats" ($feel + @{ WATER_SHOTS = "weird,pool"; WATER_CELLS = "6,3"; WATER_SECONDS = 2 })
}

# 8. Thin water on four positions of the layout.
if (Wanted "shifts") {
    foreach ($shift in "0", "0.25", "0.5", "0.75") {
        Run "shifts" $Exe "shots" ($feel + @{ WATER_SHOTS = "slope,weird"; WATER_CELLS = "6,3"; WATER_SECONDS = 3
                WATER_FPS = 1; WATER_STILLS = "3"; WATER_FILM = 0; WATER_DOTS = 0; WATER_SHIFT = $shift
                WATER_TAG = "shift$shift"
            })
    }
}

# 9. The owner's own scene in today's drawing: today's liquid beside the all-drops engine, at the
# scene's own Surface pull (Stickiness 0.5, as the trial's).
if ((Wanted "scene") -and $Scene) {
    foreach ($detail in "fine", "extreme") {
        Run "scene" $Kit "side_by_side" @{ WATER_SHOTS = "file"; WATER_SCENE = $Scene; WATER_DETAILS = $detail
            WATER_CORES = "old,drops"; WATER_SECONDS = 6; WATER_FPS = 30; WATER_SIZE = "540x960"
            WATER_MEASURE = "2,4,6"; WATER_STICK = "0.5"
        }
    }
}
"done" | Out-File -Encoding utf8 "$Out\done.txt"
