# Water mode part 2c (trapped air and Wetting) on the owner's graphics card
# (docs/superpowers/plans/water-mode/part2c-air-and-wetting.md, Tasks 4, 6 and 8). Removed from the branch before
# the merge. Run natively on Windows, from a folder outside the project, in two goes:
#
#   powershell -ExecutionPolicy Bypass -File run.ps1 -Exe <shots test exe> -Out <results folder> -Only shelves
#   powershell -ExecutionPolicy Bypass -File run.ps1 -Exe <shots test exe> -Out <results folder> -Wetting <w>
#
# (A list of groups needs -Command, as -File passes `a,b` as one string:
#   powershell -ExecutionPolicy Bypass -Command "& .\run.ps1 -Exe <exe> -Out <folder> -Wetting <w> -Only air,films")
#
# The first measures the course's top shelf at six Wettings; `<w>` for the second is the least of them whose
# tallest puddle after 30 s is at most 7 px at 6 px cells and at most 5.5 px at 3 px cells (1 if none is). Every
# run's printed table lands in <Out>\<name>.txt, stills as PNGs in <Out>, film frames in <Out>\<shot>-c<cell>[-tag]\
# and are joined into <Out>\films\<same name>.mp4 (frames deleted).
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Wetting = "0",
    [string[]]$Only = @()
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $Out, "$Out\films" | Out-Null
$names = "WATER_OUT", "WATER_SHOTS", "WATER_CELLS", "WATER_SECONDS", "WATER_FPS", "WATER_STILLS", "WATER_TAG",
    "WATER_FILM", "WATER_FILM_SCALE", "WATER_DOTS", "WATER_PULL", "WATER_STICK", "WATER_SHIFT", "WATER_FROM",
    "WATER_REST", "WATER_SLOSH", "WATER_WETTING", "WATER_WETTINGS", "WATER_BLOBS", "WATER_PLATE_SECONDS",
    "BPE_WATER_AIR", "BPE_WATER_AIR_DRAG", "BPE_WATER_DEEP"

function Wanted([string]$group) { return ($Only.Count -eq 0) -or ($Only -contains $group) }

# One run of the test exe: `$set` are its environment variables, `$test` the test's name.
function Run([string]$log, [string]$test, [hashtable]$set) {
    foreach ($n in $names) { Remove-Item "Env:$n" -ErrorAction SilentlyContinue }
    $env:WATER_OUT = $Out
    foreach ($k in $set.Keys) { Set-Item "Env:$k" ([string]$set[$k]) }
    $began = Get-Date
    & $Exe --ignored --nocapture --test-threads=1 $test 2>&1 | Out-File -Encoding utf8 -Append "$Out\$log.txt"
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

# The editor's own feel for a new Liquid (Surface pull 0.5, Stickiness 0) and the Wetting chosen.
$feel = @{ WATER_PULL = "0.5"; WATER_STICK = "0"; WATER_WETTING = $Wetting }
$seconds = @{ slope = 5; ledge = 5; pool = 6; course = 8; beads = 4; weird = 6 }

# 1. The course's top shelf as it drains, at six Wettings (Stickiness 0).
if (Wanted "shelves") {
    foreach ($w in "0", "0.3", "0.5", "0.7", "0.9", "1") {
        Run "shelves" "shelves" @{ WATER_CELLS = "6,3"; WATER_SECONDS = 30; WATER_STICK = "0"; WATER_WETTING = $w }
    }
}
if ($Only -contains "shelves") { "done" | Out-File -Encoding utf8 "$Out\done-shelves.txt"; exit 0 }

# 2. Trapped air in the pool shot at 6, 3 and 2 px, with it on and off.
if (Wanted "air") {
    Run "air" "air" ($feel + @{ WATER_SHOTS = "pool"; WATER_CELLS = "6,3,2"; WATER_SECONDS = 6 })
    Run "air" "air" ($feel + @{ WATER_SHOTS = "pool"; WATER_CELLS = "6,3,2"; WATER_SECONDS = 6; BPE_WATER_AIR = 0 })
}

# 3. Blobs and the tilted plate at five Wettings.
if (Wanted "wetting") {
    Run "wetting" "wetting" @{ WATER_CELLS = "6,3"; WATER_SECONDS = 3; WATER_BLOBS = "12,24,48" }
}

# 4. Films: the pool (air on, and off at Fine), the five other shots at Fine and Extreme, the beads at Wetting 0
# and 1 at Extreme.
if (Wanted "films") {
    foreach ($cell in "6", "3", "2") {
        Run "films" "shots" ($feel + @{ WATER_SHOTS = "pool"; WATER_CELLS = $cell; WATER_SECONDS = 6; WATER_FPS = 30
                WATER_STILLS = "3.5,4.5,5.5"; WATER_FILM_SCALE = 1; WATER_DOTS = 1 })
    }
    Run "films" "shots" ($feel + @{ WATER_SHOTS = "pool"; WATER_CELLS = "6"; WATER_SECONDS = 6; WATER_FPS = 30
            WATER_STILLS = "3.5,4.5,5.5"; WATER_FILM_SCALE = 1; WATER_DOTS = 1; BPE_WATER_AIR = 0; WATER_TAG = "airoff" })
    foreach ($cell in "6", "3") {
        foreach ($shot in "slope", "ledge", "course", "beads", "weird") {
            $s = $seconds[$shot]
            Run "films" "shots" ($feel + @{ WATER_SHOTS = $shot; WATER_CELLS = $cell; WATER_SECONDS = $s
                    WATER_FPS = 30; WATER_STILLS = "1,2,$($s - 1)"; WATER_FILM_SCALE = 1; WATER_DOTS = 0 })
        }
    }
    foreach ($w in "0", "1") {
        Run "films" "shots" @{ WATER_SHOTS = "beads"; WATER_CELLS = "3"; WATER_SECONDS = 4; WATER_FPS = 30
            WATER_STILLS = "1,3"; WATER_FILM_SCALE = 1; WATER_DOTS = 0; WATER_PULL = "0.5"; WATER_STICK = "0"
            WATER_WETTING = $w; WATER_TAG = "wet$w"
        }
    }
}

# 5. Speed: wall seconds for a scene second, physics alone, with trapped air on and off.
if (Wanted "pace") {
    foreach ($air in "1", "0") {
        Run "pace" "pace" ($feel + @{ WATER_SHOTS = "pool,course,weird"; WATER_CELLS = "6,2"; WATER_FROM = 2
                WATER_SECONDS = 1; BPE_WATER_AIR = $air })
    }
}

# 6. The deep pool at rest and its slosh.
if (Wanted "pools") {
    Run "pools" "pools" @{ WATER_CELLS = "6,3"; WATER_REST = 20; WATER_WETTING = $Wetting }
}

# 7. A hard landing's pieces (Water Weird's shot) at Surface pull 0.2 and 0.5.
if (Wanted "landing") {
    Run "landing" "landing" @{ WATER_CELLS = "6,3,2"; WATER_SECONDS = 3; WATER_WETTING = $Wetting }
}

# 8. The course: where the water is after 30 seconds.
if (Wanted "openings") {
    Run "openings" "openings" ($feel + @{ WATER_CELLS = "6,3"; WATER_SECONDS = 30 })
}

# 9. Does a run repeat: within a run of the program, and between two runs of it.
if (Wanted "repeats") {
    Run "repeats" "repeats" ($feel + @{ WATER_SHOTS = "weird,pool"; WATER_CELLS = "6,3"; WATER_SECONDS = 2 })
    Run "repeats" "repeats" ($feel + @{ WATER_SHOTS = "weird,pool"; WATER_CELLS = "6,3"; WATER_SECONDS = 2 })
}
"done" | Out-File -Encoding utf8 "$Out\done.txt"
