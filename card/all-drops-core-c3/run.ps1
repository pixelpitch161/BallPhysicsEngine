# Water mode part C3, the plain drawing, on the owner's graphics card
# (docs/superpowers/plans/water-mode/core-c3-plain-drawing.md, Task 6). The same shots as C1's card
# run (card/all-drops-core-c1/run.ps1 on branch claude/project-thread-zl1ff7), now drawn by the plain
# drawing on the card, at full size, at 6, 3 and 2 px cells; the owner's Water Weird beside today's
# liquid; the drawing's speed; the look check's water pictures. Run natively on Windows, from a
# folder outside the project:
#
#   powershell -ExecutionPolicy Bypass -File run.ps1 -Shots <shots test exe> -Plain <plain test exe>
#       -Kit <water_core test exe> -Look <full_size_look test exe> -Out <results folder>
#       -Scene "<Water Weird.ron>" [-Only films,style,scene,pace,look]
#
# Every run's printed table lands in <Out>\<name>.txt, stills as PNGs in <Out>, film frames in
# <Out>\<shot>-c<cell>[-tag]\ and are joined into <Out>\films\<same name>.mp4 (frames deleted).
param(
    [Parameter(Mandatory = $true)][string]$Shots,
    [Parameter(Mandatory = $true)][string]$Plain,
    [Parameter(Mandatory = $true)][string]$Kit,
    [Parameter(Mandatory = $true)][string]$Look,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Scene = "",
    [string[]]$Only = @()
)
$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Force -Path $Out, "$Out\films", "$Out\look" | Out-Null
$names = "WATER_OUT", "WATER_SHOTS", "WATER_CELLS", "WATER_SECONDS", "WATER_FPS", "WATER_STILLS", "WATER_TAG",
    "WATER_FILM", "WATER_FILM_SCALE", "WATER_DOTS", "WATER_PULL", "WATER_STICK", "WATER_SHIFT", "WATER_DUMP",
    "WATER_SCENE", "WATER_DETAILS", "WATER_CORES", "WATER_SIZE", "WATER_MEASURE", "LOOK_OUT", "LOOK_ONLY"

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

# C1's feel: Surface pull 0.5 (the default), Stickiness 0.5.
$feel = @{ WATER_PULL = "0.5"; WATER_STICK = "0.5" }
$seconds = @{ slope = 5; ledge = 5; pool = 6; course = 8; beads = 4; weird = 6 }

# 1. Every reference shot at 6, 3 and 2 px, films and stills at full size in the plain drawing.
if (Wanted "films") {
    foreach ($cell in "6", "3", "2") {
        foreach ($shot in "slope", "ledge", "pool", "course", "beads", "weird") {
            $s = $seconds[$shot]
            Run "films" $Shots "shots" ($feel + @{ WATER_SHOTS = $shot; WATER_CELLS = $cell; WATER_SECONDS = $s
                    WATER_FPS = 30; WATER_STILLS = "1,2,$($s - 1)"; WATER_FILM_SCALE = 1; WATER_DOTS = 0 })
        }
    }
}

# 2. Surface pull low and high, at 6 and 3 px.
if (Wanted "style") {
    foreach ($pull in "0.2", "1") {
        foreach ($cell in "6", "3") {
            Run "style" $Shots "shots" @{ WATER_SHOTS = "slope,ledge,beads"; WATER_CELLS = $cell; WATER_SECONDS = 4
                WATER_FPS = 30; WATER_STILLS = "2,3"; WATER_FILM_SCALE = 1; WATER_DOTS = 0; WATER_PULL = $pull
                WATER_STICK = "0.5"; WATER_TAG = "pull$pull"
            }
        }
    }
}

# 3. The owner's own scene: today's liquid (left) beside the all-drops engine in the plain drawing
# (right), each at full size, at the scene's own Surface pull (Stickiness 0.5, as C1's).
if ((Wanted "scene") -and $Scene) {
    foreach ($detail in "fine", "extreme") {
        Run "scene" $Kit "side_by_side" @{ WATER_SHOTS = "file"; WATER_SCENE = $Scene; WATER_DETAILS = $detail
            WATER_CORES = "old,drops"; WATER_SECONDS = 6; WATER_FPS = 30; WATER_SIZE = "1080x1920"
            WATER_MEASURE = "2,4,6"; WATER_STICK = "0.5"
        }
    }
}

# 4. The drawing's speed: ms a picture (1.4 million drops at 1080 x 1920; 86,000 at 1080 x 1920 and
# 540 x 960). Twice, to see it repeats. Nothing else may run on the card meanwhile.
if (Wanted "pace") {
    Run "pace" $Plain "plain_pace" @{}
    Run "pace" $Plain "plain_pace" @{}
}

# 5. The look check's water pictures (hand-built drops at 8, 3 and 2 px; a deep screen pool).
if (Wanted "look") {
    Run "look" $Look "render_at_full_size" @{ LOOK_OUT = "$Out\look"; LOOK_ONLY = "water" }
}
"done" | Out-File -Encoding utf8 "$Out\done.txt"
