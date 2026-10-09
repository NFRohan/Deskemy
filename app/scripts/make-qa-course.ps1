# Generate the release QA test courses (docs/release-qa.md, "Test fixtures"):
# the awkward cases real courses have, the same on every machine and release.
#
#   Deskemy QA Course     3 sections: subtitles (1 and 2 languages), chapters,
#                         2 audio tracks + an embedded subtitle (MKV), 4:3 and
#                         9:16 video, a corrupt file, a 4 s lecture, numbered
#                         and Udemy-style resources, a course-wide PDF, a cover,
#                         non-ASCII and very long names, a nested folder, a
#                         TypeScript file and a code project (not videos)
#   Deskemy QA Extra   2 plain lectures (for career tracks, library filters)
#
# Subtitle cues contain unusual words to search for: "quokka" (English),
# "ornitorrinco" (Spanish); "axolotl" is in the MKV's embedded track (shown
# in the player; search only indexes sidecar files).
#
# Needs ffmpeg on PATH (scoop install ffmpeg). Takes a minute or two; ~25 MB.
#
# Usage:  powershell -File app/scripts/make-qa-course.ps1 [-Out <folder>] [-Force]
#         -Out defaults to %USERPROFILE%\Deskemy QA. -Force replaces a folder
#         this script made before (never any other folder).

param(
    [string]$Out = (Join-Path $env:USERPROFILE "Deskemy QA"),
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$marker = ".deskemy-qa-fixture"

if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    throw "ffmpeg isn't on PATH (scoop install ffmpeg)."
}
if (Test-Path $Out) {
    if (-not $Force) { throw "$Out already exists. Use -Force to regenerate it." }
    if (-not (Test-Path (Join-Path $Out $marker))) {
        throw "$Out wasn't made by this script (no $marker); not touching it."
    }
    Remove-Item -Recurse -Force $Out
}
New-Item -ItemType Directory -Force $Out | Out-Null
Set-Content -Path (Join-Path $Out $marker) -Value "Made by app/scripts/make-qa-course.ps1"

$work = Join-Path $Out ".work"
New-Item -ItemType Directory -Force $work | Out-Null

# UTF-8 without a BOM (Windows PowerShell 5.1's Set-Content can't).
function Write-Text([string]$path, [string]$text) {
    [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding $false))
}

function Invoke-Ffmpeg([string[]]$ffArgs) {
    & ffmpeg -hide_banner -loglevel error -y @ffArgs
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg failed: $($ffArgs -join ' ')" }
}

# A test pattern with a running clock and a tone. Encoded under an ASCII name
# in .work, then moved into place (any name, any folder).
function New-Video {
    param([string]$dest, [int]$seconds, [string]$size = "640x360", [int]$tone = 440,
          [string]$pattern = "testsrc2", [string[]]$extra = @())
    $tmp = Join-Path $work ("v" + [guid]::NewGuid().ToString("N") + [System.IO.Path]::GetExtension($dest))
    Invoke-Ffmpeg (@(
        "-f", "lavfi", "-i", "${pattern}=size=${size}:rate=24:duration=$seconds",
        "-f", "lavfi", "-i", "sine=frequency=${tone}:duration=$seconds"
    ) + $extra + @(
        "-c:v", "libx264", "-preset", "ultrafast", "-crf", "32", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "48k", "-shortest", $tmp
    ))
    New-Item -ItemType Directory -Force (Split-Path $dest -Parent) | Out-Null
    Move-Item -Force $tmp $dest
}

function New-Srt([string]$path, [string[]]$lines) {
    $i = 0
    $cues = foreach ($line in $lines) {
        $i++
        "{0}`n00:00:{1:00},000 --> 00:00:{2:00},000`n{3}`n" -f $i, (($i - 1) * 3), ($i * 3), $line
    }
    Write-Text $path ($cues -join "`n")
}

# The smallest valid one-page PDF showing a line of text.
function New-Pdf([string]$path, [string]$text) {
    $stream = "BT /F1 18 Tf 72 720 Td ($text) Tj ET"
    $objects = @(
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Length $($stream.Length) >>`nstream`n$stream`nendstream"
    )
    $body = "%PDF-1.4`n"
    $offsets = @()
    for ($n = 0; $n -lt $objects.Count; $n++) {
        $offsets += $body.Length
        $body += "$($n + 1) 0 obj`n$($objects[$n])`nendobj`n"
    }
    $xref = $body.Length
    $body += "xref`n0 $($objects.Count + 1)`n0000000000 65535 f `n"
    foreach ($o in $offsets) { $body += ("{0:0000000000} 00000 n `n" -f $o) }
    $body += "trailer`n<< /Size $($objects.Count + 1) /Root 1 0 R >>`nstartxref`n$xref`n%%EOF`n"
    [System.IO.File]::WriteAllText($path, $body, [System.Text.Encoding]::ASCII)
}

$course = Join-Path $Out "Deskemy QA Course"
$s1 = Join-Path $course "01 Getting Started"
$s2 = Join-Path $course "02 Formats"
$s3 = Join-Path $course "03 Edge Cases"
foreach ($d in $course, $s1, $s2, $s3) { New-Item -ItemType Directory -Force $d | Out-Null }

Write-Host "Course 1: cover and course-wide resource"
Invoke-Ffmpeg @("-f", "lavfi", "-i", "testsrc2=size=1280x720", "-frames:v", "1", (Join-Path $course "cover.jpg"))
New-Pdf (Join-Path $course "Course-wide cheatsheet.pdf") "Deskemy QA - course-wide cheatsheet"

Write-Host "Section 1: subtitles, a resource between lectures, chapters, a short clip"
New-Video (Join-Path $s1 "001 Welcome.mp4") 12
New-Srt (Join-Path $s1 "001 Welcome.en.srt") @("Welcome to the QA course.", "Search for the word quokka.", "This line is the last one.")
New-Video (Join-Path $s1 "002 Two Subtitle Languages.mp4") 12 -tone 523
New-Srt (Join-Path $s1 "002 Two Subtitle Languages.en.srt") @("English track, cue one.", "English track, cue two.")
New-Srt (Join-Path $s1 "002 Two Subtitle Languages.es.srt") @("Pista en espanol, uno.", "Busca la palabra ornitorrinco.")
Write-Text (Join-Path $s1 "003 Configuring Git.html") "<!doctype html><title>Configuring Git</title><h1>003 Configuring Git</h1><p>A numbered article: it belongs between lectures 2 and 4.</p>"
# Six minutes: the Chapters button once vanished a few minutes into a video.
$meta = Join-Path $work "chapters.txt"
$lines = @(";FFMETADATA1")
$starts = @(0, 60, 150, 270, 360); $titles = @("Intro", "Setup", "Deep dive", "Wrap-up")
for ($k = 0; $k -lt 4; $k++) {
    $lines += "[CHAPTER]", "TIMEBASE=1/1000", "START=$($starts[$k] * 1000)", "END=$($starts[$k + 1] * 1000)", "title=$($titles[$k])"
}
Write-Text $meta (($lines -join "`n") + "`n")
New-Video (Join-Path $s1 "004 Chapters.mp4") 360 -size "426x240" -extra @("-i", $meta, "-map", "0:v", "-map", "1:a", "-map_chapters", "2")
New-Video (Join-Path $s1 "005 Short Clip.mp4") 4 -tone 660

Write-Host "Section 2: two audio tracks, 4:3, 9:16, a corrupt file, a Udemy-style resource"
$embedded = Join-Path $work "embedded.srt"
New-Srt $embedded @("Embedded subtitle track.", "The secret word is axolotl.")
$mkv = Join-Path $work "two-audio.mkv"
Invoke-Ffmpeg @(
    "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=24:duration=20",
    "-f", "lavfi", "-i", "sine=frequency=440:duration=20",
    "-f", "lavfi", "-i", "sine=frequency=880:duration=20",
    "-i", $embedded,
    "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s",
    "-c:v", "libx264", "-preset", "ultrafast", "-crf", "32", "-pix_fmt", "yuv420p",
    "-c:a", "aac", "-b:a", "48k", "-c:s", "srt",
    "-metadata:s:a:0", "language=eng", "-metadata:s:a:0", "title=English (440 Hz)",
    "-metadata:s:a:1", "language=spa", "-metadata:s:a:1", "title=Spanish (880 Hz)",
    # No -shortest: the subtitle stream ends at its last cue (6 s).
    "-metadata:s:s:0", "language=eng", "-t", "20", $mkv
)
Move-Item $mkv (Join-Path $s2 "006 Two Audio Tracks.mkv")
New-Video (Join-Path $s2 "007 Classic 4x3.mp4") 15 -size "640x480" -pattern "smptebars"
New-Video (Join-Path $s2 "008 Vertical 9x16.mp4") 15 -size "360x640" -pattern "testsrc"
$junk = New-Object byte[] 65536
(New-Object System.Random 7).NextBytes($junk)
[System.IO.File]::WriteAllBytes((Join-Path $s2 "009 Corrupt.mp4"), $junk)
New-Pdf (Join-Path $s2 "8.1 Slides.pdf") "Lecture 8 slides (Udemy-style 8.1 numbering)"

Write-Host "Section 3: non-ASCII and long names, nesting, files that aren't videos"
# "Über Café – 第1章 (final)", spelled with char codes so this script stays ASCII.
$intl = "010 " + [char]0x00DC + "ber Caf" + [char]0x00E9 + " " + [char]0x2013 + " " + [char]0x7B2C + "1" + [char]0x7AE0 + " (final).mp4"
New-Video (Join-Path $s3 $intl) 10 -tone 330
New-Video (Join-Path $s3 "011 A very long lecture title that goes on and on to test eliding in the curriculum, the player title bar and the mini player.mp4") 10 -tone 392
New-Video (Join-Path $s3 "Extras\Even deeper\012 Nested Lecture.mp4") 8 -tone 494
Write-Text (Join-Path $s3 "utils.ts") "export const answer = 42;`n"
$code = Join-Path $s3 "code-project"
New-Item -ItemType Directory -Force (Join-Path $code "src") | Out-Null
Write-Text (Join-Path $code "package.json") "{ `"name`": `"qa-project`", `"version`": `"1.0.0`" }`n"
Write-Text (Join-Path $code "src\index.ts") "import { answer } from `"../../utils`";`nconsole.log(answer);`n"
Write-Text (Join-Path $code "src\app.ts") "export function app(): void {}`n"

Write-Host "Course 2: two plain lectures"
$course2 = Join-Path $Out "Deskemy QA Extra"
New-Video (Join-Path $course2 "01 First Lecture.mp4") 10 -pattern "rgbtestsrc" -tone 349
New-Video (Join-Path $course2 "02 Second Lecture.mp4") 10 -pattern "rgbtestsrc" -tone 440

Remove-Item -Recurse -Force $work
$mb = [math]::Round(((Get-ChildItem $Out -Recurse -File | Measure-Object Length -Sum).Sum) / 1MB, 1)
Write-Host "Done: $Out ($mb MB). Add each course with Add Folder, or the folder itself as a library root."
