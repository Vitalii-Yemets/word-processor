# Opens documents in Microsoft Word, says whether Word took them as they are,
# and hands Word's own pages to the fidelity comparison.
#
# Why: two questions nothing in the build image can ask. Does Word open a file
# this program wrote without offering to repair it — the one test of a writer
# that counts, since Word is what the files are written for — and what does
# Word's page of a document look like, which is what `wp fidelity` scores this
# program's pages against. Word is on the Windows host and not in the
# container, so this runs on the host, through Word's COM automation, and is
# not a test in `cargo test`.
#
# For each document it opens it read-only with alerts off, macros forced off
# and Word's repair not asked for, and the verdict says what came of that:
#
#   opened    it opened.
#   repaired  it would not open as it is — COM says "The file appears to be
#             corrupted", Word error 5792, where the window would have said
#             "Word found unreadable content ... Do you want to recover the
#             contents?" — and opened when repair was asked for. Asking for it
#             always gives an untitled copy, so a repaired file is told by the
#             first open failing and the second succeeding, not by anything
#             about the document that comes back.
#   refused   it would not open either way; for a file Word cannot make sense
#             of, COM says "Word experienced an error trying to open the
#             file", Word error 5121.
#   timeout   Word did not finish with it in time. The Word is ended by its
#             process number and a fresh one started for the next file.
#   crashed   Word went away while working on it, and a fresh one is started.
#
# A document that opened, or was repaired, is exported as a PDF — Word's own
# printed page — to reference\<document>.pdf, and its pages are drawn from
# that PDF into reference\<document>\page-1.png and on, which is where
# `wp fidelity` looks for Word's pages (see corpus/README.md). The drawing is
# the PDF renderer Windows has, not Word's: Word gives a page as a picture only
# as a metafile, and GDI+ plays a metafile's text back heavier than Word drew
# it, so a PDF of Word's page drawn by a real PDF renderer is the more faithful
# of the two. Then `wp fidelity` is run over the directory and each document's
# score is put beside its verdict.
#
# Nothing is ever saved through Word: every document is opened read-only and
# closed without saving, and Word is told to quit without saving. The Word it
# starts is ended by its process number if it outlives Quit, which it does
# while anything here still holds a reference to it.
#
# Run on the Windows host:
#
#   powershell -File tools\word-check.ps1 corpus
#   powershell -File tools\word-check.ps1 a.docx b.rtf -Report .tmpwork\check
#
# The report is written as text and as JSON, to reference\word-check.txt and
# .json in the directory unless -Report says where.

param(
    # A directory of documents, or the documents themselves.
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Path,
    # How long one document may take, in seconds, before Word is ended.
    [int]$Timeout = 120,
    # The resolution Word's pages are drawn at.
    [int]$Dpi = 150,
    # The wp.exe that scores the pages; dist\wp.exe if not given.
    [string]$Wp = '',
    # Leave the scoring out.
    [switch]$NoScore,
    # Where the report goes, without its extension.
    [string]$Report = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

# Numbers and times are written the same whatever the machine's language.
$invariant = [Globalization.CultureInfo]::InvariantCulture

$repository = Split-Path -Parent $PSScriptRoot
$wordExe = 'WINWORD'

# The kinds Word is asked to open: Word's four packages, the old binary
# format, Rich Text and OpenDocument.
$kinds = @('.docx', '.docm', '.dotx', '.dotm', '.doc', '.rtf', '.odt')

# Word's constants, named where they are used.
$wdAlertsNone = 0
$msoAutomationSecurityForceDisable = 3
$wdDoNotSaveChanges = 0
$wdExportFormatPDF = 17
$wdStatisticPages = 2

# --- Finding the documents ----------------------------------------------------

# Every document under a directory, the way `wp corpus` finds them: hidden
# files and Word's `~$` lock files left alone, the reference folder at the top
# skipped because it holds Word's pages and not documents, and no deeper than
# eight folders.
function Find-Documents([string]$directory, [int]$depth) {
    if ($depth -gt 8) { return }
    foreach ($entry in Get-ChildItem -LiteralPath $directory -Force | Sort-Object Name) {
        if ($entry.Name.StartsWith('.') -or $entry.Name.StartsWith('~$')) { continue }
        if ($depth -eq 0 -and $entry.Name -eq 'reference') { continue }
        if ($entry.PSIsContainer) {
            Find-Documents $entry.FullName ($depth + 1)
        } elseif ($kinds -contains $entry.Extension.ToLowerInvariant()) {
            $entry.FullName
        }
    }
}

# Each document with the directory it is measured in: the one it was found
# under, or its own folder when it was named by itself.
$work = New-Object System.Collections.Generic.List[object]
foreach ($given in $Path) {
    $full = (Resolve-Path -LiteralPath $given).ProviderPath
    if (Test-Path -LiteralPath $full -PathType Container) {
        foreach ($document in @(Find-Documents $full 0)) {
            $work.Add([pscustomobject]@{ Root = $full; File = $document })
        }
    } else {
        $work.Add([pscustomobject]@{ Root = (Split-Path -Parent $full); File = $full })
    }
}
if ($work.Count -eq 0) {
    Write-Host "No documents in $($Path -join ', ')."
    exit 2
}

function Get-Relative([string]$root, [string]$file) {
    $file.Substring($root.TrimEnd('\').Length + 1)
}

# Where Word's PDF and its pages go for a document: beside the reference
# folder `wp fidelity` reads, and in it.
function Get-ReferenceBase([string]$root, [string]$file) {
    $relative = Get-Relative $root $file
    $stem = [IO.Path]::Combine([IO.Path]::GetDirectoryName($relative),
        [IO.Path]::GetFileNameWithoutExtension($relative))
    Join-Path (Join-Path $root 'reference') $stem
}

# --- Word ---------------------------------------------------------------------

# Calls a method of a COM object with its parameters named.
#
# By name because Word's methods take a dozen optional parameters by
# reference, which PowerShell will not pass by position, and because a call
# that says OpenAndRepair is one a reader can check. Values are unwrapped
# first: a string PowerShell has wrapped is a type Word does not accept.
# A Word that is busy says so rather than failing, and is asked again.
function Invoke-Word($target, [string]$method, [hashtable]$named) {
    $names = [string[]]@($named.Keys)
    $values = New-Object 'object[]' $names.Count
    for ($i = 0; $i -lt $names.Count; $i++) {
        $value = $named[$names[$i]]
        if ($value -is [psobject]) { $value = $value.psobject.BaseObject }
        $values[$i] = $value
    }
    for ($attempt = 0; ; $attempt++) {
        try {
            return $target.GetType().InvokeMember($method,
                [Reflection.BindingFlags]::InvokeMethod, $null, $target, $values, $null, $null, $names)
        } catch {
            $code = (Get-Innermost $_.Exception).HResult
            # RPC_E_CALL_REJECTED and RPC_E_SERVERCALL_RETRYLATER: busy.
            $busy = ($code -eq -2147418111) -or ($code -eq -2147417846)
            if (-not $busy -or $attempt -ge 50 -or $watch.Fired) { throw }
            Start-Sleep -Milliseconds 200
        }
    }
}

function Get-Innermost([Exception]$exception) {
    while ($exception.InnerException) { $exception = $exception.InnerException }
    $exception
}

# What Word said, with its error number: the low half of the HRESULT is
# Word's own number for the error, the one its documentation uses.
function Get-WordError($record) {
    $exception = Get-Innermost $record.Exception
    $text = $exception.Message.Trim()
    if (($exception.HResult -band 0xFFFF0000) -eq 0x800A0000) {
        $text += " (Word error $($exception.HResult -band 0xFFFF))"
    }
    $text
}

# A watchdog on a thread of its own: while a document is being worked on it
# holds a deadline, and a Word still busy past it is ended by its process
# number. That is the only thing that returns a COM call Word never answers —
# the call fails once the process is gone, and the run goes on.
$watch = [hashtable]::Synchronized(@{ Pid = 0; Deadline = [datetime]::MaxValue; Fired = $false; Stop = $false })
$watchdog = [PowerShell]::Create()
[void]$watchdog.AddScript({
    param($watch)
    while (-not $watch.Stop) {
        if ($watch.Pid -ne 0 -and [datetime]::UtcNow -gt $watch.Deadline) {
            $watch.Deadline = [datetime]::MaxValue
            $watch.Fired = $true
            try { [Diagnostics.Process]::GetProcessById($watch.Pid).Kill() } catch { }
        }
        Start-Sleep -Milliseconds 200
    }
}).AddArgument($watch)

$word = $null
$wordPid = 0
$version = ''

# Starts a Word and learns its process number.
#
# Word started for automation runs as `WINWORD.EXE /Automation -Embedding`,
# so the process that is new since a moment ago and has that on its command
# line is this one, and not a Word somebody has open.
function Start-Word {
    $before = @(Get-Process $wordExe -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    $script:word = New-Object -ComObject Word.Application
    $new = @(Get-CimInstance Win32_Process -Filter "Name = 'WINWORD.EXE'" |
        Where-Object { $before -notcontains $_.ProcessId -and $_.CommandLine -match '/Automation' })
    $script:wordPid = if ($new.Count -eq 1) { [int]$new[0].ProcessId } else { 0 }
    $watch.Pid = $script:wordPid
    # No prompts, and no macros run: a document's AutoOpen runs under
    # automation unless it is forced off, since automation's default is to
    # trust whatever it opens.
    $script:word.DisplayAlerts = $wdAlertsNone
    $script:word.AutomationSecurity = $msoAutomationSecurityForceDisable
    $script:version = "$($script:word.Version) (build $($script:word.Build))"
    $pidSaid = if ($script:wordPid) { "process $($script:wordPid)" } else { 'process not found' }
    Write-Host "Word $($script:version), $pidSaid"
}

# Lets Word go: quits it without saving, and ends the process by its number
# if it is still there a few seconds after every reference to it is dropped.
# Quit is watched like a document is, since a Word can hang there too.
function Stop-Word {
    if ($script:word) {
        $watch.Deadline = [datetime]::UtcNow.AddSeconds(30)
        try { [void](Invoke-Word $script:word 'Quit' @{ SaveChanges = $wdDoNotSaveChanges }) } catch { }
        $watch.Deadline = [datetime]::MaxValue
        try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:word) } catch { }
        $script:word = $null
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }
    if ($script:wordPid) {
        $process = Get-Process -Id $script:wordPid -ErrorAction SilentlyContinue
        if ($process -and -not $process.WaitForExit(8000)) {
            Stop-Process -Id $script:wordPid -Force -ErrorAction SilentlyContinue
            Write-Host "  (Word outlived Quit: process $($script:wordPid) ended)"
        }
    }
    $script:wordPid = 0
    $watch.Pid = 0
}

# Opens a document read-only, with or without Word's repair.
#
# A password that cannot be right is given, so that a document with one fails
# at once instead of waiting for somebody to type it.
function Open-Document([string]$file, [bool]$repair) {
    Invoke-Word $script:word.Documents 'Open' @{
        FileName = $file; ConfirmConversions = $false; ReadOnly = $true
        AddToRecentFiles = $false; PasswordDocument = 'word-check has no password'
        PasswordTemplate = 'word-check has no password'
        WritePasswordDocument = 'word-check has no password'
        Visible = $false; OpenAndRepair = $repair; NoEncodingDialog = $true
    }
}

# --- Word's pages as pictures -------------------------------------------------

# Windows' own PDF renderer, reached through its runtime from PowerShell.
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Data.Pdf.PdfDocument, Windows.Data.Pdf, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.InMemoryRandomAccessStream, Windows.Storage.Streams, ContentType = WindowsRuntime]
$pngEncoder = [Windows.Graphics.Imaging.BitmapEncoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime]::PngEncoderId
$asTask = [WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$asActionTask = [WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncAction' } | Select-Object -First 1

function Wait-Operation($operation, [Type]$type) {
    $task = $asTask.MakeGenericMethod($type).Invoke($null, @($operation))
    if (-not $task.Wait(60000)) { throw 'the PDF renderer did not answer' }
    $task.Result
}

function Wait-Action($action) {
    if (-not $asActionTask.Invoke($null, @($action)).Wait(60000)) {
        throw 'the PDF renderer did not answer'
    }
}

# Draws every page of a PDF into a folder as page-1.png and on, at a whole
# number of dots to the inch, and says how many there were. Pictures a run
# before left there are removed first: a document that has lost a page since
# must not be scored against the page it lost.
function Export-Pages([string]$pdf, [string]$folder, [int]$dpi) {
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    Get-ChildItem -LiteralPath $folder -Filter 'page-*.png' | ForEach-Object {
        Remove-Item -LiteralPath $_.FullName
    }
    $file = Wait-Operation ([Windows.Storage.StorageFile]::GetFileFromPathAsync($pdf)) ([Windows.Storage.StorageFile])
    $document = Wait-Operation ([Windows.Data.Pdf.PdfDocument]::LoadFromFileAsync($file)) ([Windows.Data.Pdf.PdfDocument])
    for ($index = 0; $index -lt $document.PageCount; $index++) {
        $page = $document.GetPage($index)
        try {
            # A page's size is given in ninety-sixths of an inch.
            $options = New-Object Windows.Data.Pdf.PdfPageRenderOptions
            $options.DestinationWidth = [uint32][Math]::Round($page.Size.Width / 96 * $dpi)
            $options.DestinationHeight = [uint32][Math]::Round($page.Size.Height / 96 * $dpi)
            $options.BitmapEncoderId = $pngEncoder
            $stream = New-Object Windows.Storage.Streams.InMemoryRandomAccessStream
            Wait-Action ($page.RenderToStreamAsync($stream, $options))
            $source = [IO.WindowsRuntimeStreamExtensions]::AsStreamForRead($stream.GetInputStreamAt(0))
            $target = [IO.File]::Create((Join-Path $folder "page-$($index + 1).png"))
            try { $source.CopyTo($target) } finally { $target.Dispose(); $source.Dispose(); $stream.Dispose() }
        } finally {
            $page.Dispose()
        }
    }
    $document.PageCount
}

# --- One document -------------------------------------------------------------

# The reference folders this run has filled, and for whom. `wp fidelity` names
# a document's folder after the document without its extension, so report.docx
# and report.rtf beside it share one; the second is opened and judged, but its
# pages are not drawn over the first's and it is not scored.
$claimed = @{}

function Test-Document($item) {
    $relative = Get-Relative $item.Root $item.File
    $record = [ordered]@{
        file = $relative; path = $item.File; verdict = ''; error = $null
        pdf = $null; pages = $null; images = 0; seconds = 0.0
        tolerant = $null; exact = $null; fidelity = $null
    }
    $base = Get-ReferenceBase $item.Root $item.File
    $owner = $claimed[$base.ToLowerInvariant()]
    if (-not $owner) { $claimed[$base.ToLowerInvariant()] = $relative }
    $started = [datetime]::UtcNow
    if (-not $script:word) { Start-Word }
    $watch.Fired = $false
    $watch.Deadline = [datetime]::UtcNow.AddSeconds($Timeout)
    $document = $null
    try {
        try {
            $document = Open-Document $item.File $false
            $record.verdict = 'opened'
        } catch {
            if ($watch.Fired) { throw }
            $record.error = Get-WordError $_
            try {
                $document = Open-Document $item.File $true
                $record.verdict = 'repaired'
            } catch {
                if ($watch.Fired) { throw }
                $record.verdict = 'refused'
            }
        }
        if ($document) {
            $record.pages = [int]$document.ComputeStatistics($wdStatisticPages)
        }
        if ($document -and $owner) {
            $record.error = "pages not drawn: reference\$(Get-Relative (Join-Path $item.Root 'reference') $base) is $owner's"
        } elseif ($document) {
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $base) | Out-Null
            $pdf = "$base.pdf"
            try {
                [void](Invoke-Word $document 'ExportAsFixedFormat' @{
                    OutputFileName = $pdf; ExportFormat = $wdExportFormatPDF; OpenAfterExport = $false
                })
                $record.pdf = Get-Relative $item.Root $pdf
            } catch {
                if ($watch.Fired) { throw }
                $record.error = "export: $(Get-WordError $_)"
            }
        }
        if ($document) {
            [void](Invoke-Word $document 'Close' @{ SaveChanges = $wdDoNotSaveChanges })
            $document = $null
        }
    } catch {
        $code = (Get-Innermost $_.Exception).HResult
        # RPC_S_SERVER_UNAVAILABLE, RPC_S_CALL_FAILED and RPC_E_DISCONNECTED:
        # the Word at the other end of the call is not there any more.
        $gone = @(-2147023174, -2147023170, -2147417848) -contains $code
        if ($watch.Fired) {
            $record.verdict = 'timeout'
            $record.error = "Word did not finish with it in $Timeout s and was ended"
        } elseif ($gone) {
            if (-not $record.verdict -or $record.verdict -eq 'opened') { $record.verdict = 'crashed' }
            $record.error = "Word went away while working on it: $(Get-WordError $_)"
        } else {
            if (-not $record.verdict) { $record.verdict = 'refused' }
            $record.error = Get-WordError $_
            if ($document) {
                try { [void](Invoke-Word $document 'Close' @{ SaveChanges = $wdDoNotSaveChanges }) } catch { }
            }
        }
        if ($watch.Fired -or $gone) {
            # That Word is finished with; a new one is started for the next
            # document, and whatever is left of this one is ended.
            try { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($script:word) } catch { }
            $script:word = $null
            if ($script:wordPid) { Stop-Process -Id $script:wordPid -Force -ErrorAction SilentlyContinue }
            $script:wordPid = 0
            $watch.Pid = 0
        }
    } finally {
        $watch.Deadline = [datetime]::MaxValue
    }

    # The pages are drawn once Word has let go of the file.
    if ($record.pdf) {
        try {
            $record.images = Export-Pages (Join-Path $item.Root $record.pdf) $base $Dpi
        } catch {
            $record.error = "pages: $($_.Exception.Message)"
        }
    }
    $record.seconds = [Math]::Round(([datetime]::UtcNow - $started).TotalSeconds, 1)
    [pscustomobject]$record
}

# --- The run ------------------------------------------------------------------

$records = New-Object System.Collections.Generic.List[object]
$watchdogRun = $watchdog.BeginInvoke()
try {
    foreach ($item in $work) {
        $record = Test-Document $item
        $records.Add($record)
        $said = "$($record.verdict), $($record.images) page(s)"
        if ($record.error) { $said += " - $($record.error -replace '\s*[\r\n]+\s*', ' ')" }
        Write-Host ("  {0}  {1}" -f $record.file, $said)
    }
} finally {
    Stop-Word
    $watch.Stop = $true
    [void]$watchdog.EndInvoke($watchdogRun)
    $watchdog.Dispose()
    # The PDF renderer keeps the files it read open until it is collected.
    [GC]::Collect()
    [GC]::WaitForPendingFinalizers()
}

# --- The score ----------------------------------------------------------------

# `wp fidelity` over each directory, and every document's line of it put with
# the document. Its output is UTF-8, which the console is told so that a name
# outside ASCII comes through as it is.
$scored = @()
if (-not $NoScore) {
    if (-not $Wp) { $Wp = Join-Path $repository 'dist\wp.exe' }
    if (Test-Path -LiteralPath $Wp) {
        $Wp = (Resolve-Path -LiteralPath $Wp).ProviderPath
        $encoding = [Console]::OutputEncoding
        [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
        # From the repository, whose commit `wp fidelity` writes into its
        # history; and with errors let through, because what it says on
        # standard error is part of its report and not a reason to stop.
        Push-Location $repository
        $ErrorActionPreference = 'Continue'
        try {
            foreach ($root in @($work | ForEach-Object { $_.Root } | Sort-Object -Unique)) {
                Write-Host ''
                $lines = @(& $Wp fidelity $root 2>&1 | ForEach-Object { "$_" })
                $lines | ForEach-Object { Write-Host $_ }
                $scored += $lines
                # Only a document whose pages this run drew is given a score:
                # the pictures in any other's folder are not its own, or not
                # of today.
                foreach ($record in $records | Where-Object { $_.pdf -and $_.images -gt 0 -and $_.path.StartsWith($root) }) {
                    $name = Get-Relative $root $record.path
                    $line = $lines | Where-Object { $_ -match ('^  ' + [regex]::Escape($name) + '\s{2,}(.*)$') } |
                        Select-Object -First 1
                    if (-not $line) { continue }
                    $detail = ([regex]::Match($line, '^  ' + [regex]::Escape($name) + '\s{2,}(.*)$')).Groups[1].Value
                    $record.fidelity = $detail
                    $score = [regex]::Match($detail, '([\d.]+)% tolerant, ([\d.]+)% exact')
                    if ($score.Success) {
                        $record.tolerant = [double]::Parse($score.Groups[1].Value, $invariant)
                        $record.exact = [double]::Parse($score.Groups[2].Value, $invariant)
                    }
                }
            }
        } finally {
            $ErrorActionPreference = 'Stop'
            Pop-Location
            [Console]::OutputEncoding = $encoding
        }
    } else {
        Write-Host "(not scored: no $Wp - build it with .\x.ps1 win)"
    }
}

# --- The report ---------------------------------------------------------------

if (-not $Report) { $Report = Join-Path (Join-Path $work[0].Root 'reference') 'word-check' }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Report) | Out-Null

$stamp = [datetime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ', $invariant)
$widest = [Math]::Max(4, ($records | ForEach-Object { $_.file.Length } | Measure-Object -Maximum).Maximum)
$text = New-Object System.Collections.Generic.List[string]
$text.Add("Word $version, $stamp, $($records.Count) document(s), pages at $Dpi dpi")
$text.Add('')
$text.Add(("{0,-$widest}  {1,-8}  {2,5}  {3,8}  {4,6}  {5}" -f 'FILE', 'VERDICT', 'PAGES', 'TOLERANT', 'EXACT', 'PDF / ERROR'))
foreach ($record in $records) {
    $pages = if ($null -ne $record.pages) { "$($record.pages)" } else { '-' }
    $tolerant = if ($null -ne $record.tolerant) { $record.tolerant.ToString('0.0', $invariant) + '%' } else { '-' }
    $exact = if ($null -ne $record.exact) { $record.exact.ToString('0.0', $invariant) + '%' } else { '-' }
    $last = @()
    if ($record.pdf) { $last += $record.pdf }
    # The first sentence is enough in a line; the JSON has the whole of it.
    if ($record.error) {
        # Word separates the lines of a message with a carriage return alone.
        $said = ($record.error -replace '\s*[\r\n]+\s*', ' ')
        $first = ($said -split '(?<=\.)\s')[0]
        $number = [regex]::Match($said, '\(Word error \d+\)$').Value
        if ($number -and -not $first.EndsWith($number)) { $first += " $number" }
        $last += $first
    }
    $text.Add(("{0,-$widest}  {1,-8}  {2,5}  {3,8}  {4,6}  {5}" -f $record.file, $record.verdict, $pages, $tolerant, $exact, ($last -join '  ')))
}
$counts = $records | Group-Object verdict | Sort-Object Name | ForEach-Object { "$($_.Count) $($_.Name)" }
$text.Add('')
$text.Add(($counts -join ', '))
if ($scored.Count -gt 0) {
    $text.Add('')
    $text.Add('wp fidelity:')
    foreach ($line in $scored) { $text.Add($line) }
}

$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText("$Report.txt", ($text -join "`r`n") + "`r`n", $utf8)
$json = [ordered]@{ word = $version; when = $stamp; dpi = $Dpi; documents = $records.ToArray() }
[IO.File]::WriteAllText("$Report.json", (ConvertTo-Json $json -Depth 4), $utf8)

Write-Host ''
Write-Host ($counts -join ', ')
Write-Host "$Report.txt"
Write-Host "$Report.json"

# A document Word would not take as it is, is the thing this exists to find.
$bad = @($records | Where-Object { $_.verdict -ne 'opened' }).Count
if ($bad -gt 0) { exit 1 }
exit 0
