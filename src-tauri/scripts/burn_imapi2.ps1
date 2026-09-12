<#
.SYNOPSIS
    SpotyBurn IMAPI2 Driver Script for Windows
.DESCRIPTION
    Interfaces with Windows Image Mastering API v2 (IMAPI2) via COM
    for drive enumeration, disc status, Red Book Audio CD burning,
    Data CD burning, and disc ejection.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("list", "status", "burn-audio", "burn-data", "eject")]
    [string]$Action,

    [string]$DriveId = "0",
    [string]$SourcePath = "",
    [int]$Speed = 0,
    [switch]$Eject = $false,
    [switch]$Simulate = $false
)

$ErrorActionPreference = "Stop"

function Get-DiscMaster {
    try {
        return New-Object -ComObject IMAPI2.MsftDiscMaster2
    } catch {
        Write-Error "IMAPI2 is not available on this system: $_"
        exit 1
    }
}

function List-Drives {
    $dm = Get-DiscMaster
    $drives = @()
    for ($i = 0; $i -lt $dm.Count; $i++) {
        $id = $dm.Item($i)
        $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
        $rec.InitializeDiscRecorder($id)
        
        $interconnect = "ATAPI"
        $driveLetter = ""
        try {
            if ($rec.VolumePathNames -and $rec.VolumePathNames.Count -gt 0) {
                $driveLetter = $rec.VolumePathNames[0]
            }
        } catch {
            $driveLetter = ""
        }
        
        $vendor = ""
        $product = ""
        try {
            if ($rec.VendorId) { $vendor = ($rec.VendorId -as [string]).Trim() }
            if ($rec.ProductId) { $product = ($rec.ProductId -as [string]).Trim() }
        } catch {}
        
        $drives += [PSCustomObject]@{
            id = "$i"
            vendor = $vendor
            product = $product
            interconnect = $interconnect
            path = $driveLetter
        }
    }
    $drives | ConvertTo-Json -Compress
}

function Get-DriveStatus {
    param([string]$TargetId)
    $dm = Get-DiscMaster
    $idx = [int]$TargetId
    if ($idx -lt 0 -or $idx -ge $dm.Count) {
        Write-Error "Drive index $TargetId out of range"
        exit 1
    }
    
    $id = $dm.Item($idx)
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($id)
    
    $format = New-Object -ComObject IMAPI2.MsftDiscFormat2Data
    $format.Recorder = $rec
    
    $mediaPresent = $false
    $isBlank = $false
    $mediaType = "None"
    $freeBlocks = 0
    $freeMinutes = 0.0
    
    try {
        if ($format.IsCurrentMediaSupported($rec)) {
            $mediaPresent = $true
            $isBlank = [bool]$format.MediaPhysicallyBlank
            $freeBlocks = [int64]$format.FreeSectorsOnMedia
            # 75 sectors per second, 60 seconds per minute = 4500 sectors/min
            $freeMinutes = [double]$freeBlocks / 4500.0
            
            switch ($format.CurrentPhysicalMediaType) {
                0 { $mediaType = "Unknown" }
                1 { $mediaType = "CD-ROM" }
                2 { $mediaType = "CD-R" }
                3 { $mediaType = "CD-RW" }
                4 { $mediaType = "DVD-ROM" }
                5 { $mediaType = "DVD-RAM" }
                6 { $mediaType = "DVD-R" }
                7 { $mediaType = "DVD-RW" }
                default { $mediaType = "CD-R" }
            }
        }
    } catch {
        $mediaPresent = $false
    }
    
    $res = [PSCustomObject]@{
        drive_id = $TargetId
        media_present = $mediaPresent
        is_blank = $isBlank
        media_type = $mediaType
        free_blocks = $freeBlocks
        free_minutes = [Math]::Round($freeMinutes, 2)
    }
    $res | ConvertTo-Json -Compress
}

function Burn-Audio {
    param(
        [string]$TargetId,
        [string]$Source,
        [int]$WriteSpeed,
        [bool]$DoEject
    )
    $dm = Get-DiscMaster
    $idx = [int]$TargetId
    $id = $dm.Item($idx)
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($id)
    
    $tao = New-Object -ComObject IMAPI2.MsftDiscFormat2TrackAtOnce
    $tao.Recorder = $rec
    $tao.ClientName = "SpotyBurn"
    
    Write-Output "PROGRESS:stage=Preparing,percent=5.0,message=Preparing audio tracks"
    
    $files = @()
    if (Test-Path -Path $Source -PathType Container) {
        $files = Get-ChildItem -Path $Source -Filter "*.wav" | Sort-Object Name
    } elseif (Test-Path -Path $Source -PathType Leaf) {
        $files = @(Get-Item -Path $Source)
    } else {
        Write-Error "Source path not found: $Source"
        exit 1
    }
    
    if ($files.Count -eq 0) {
        Write-Error "No audio files found at $Source"
        exit 1
    }
    
    $tao.PrepareMedia()
    $totalTracks = $files.Count
    $currentTrack = 0
    
    foreach ($file in $files) {
        $currentTrack++
        $pct = [Math]::Round(($currentTrack / $totalTracks) * 85.0 + 5.0, 1)
        Write-Output "PROGRESS:stage=Writing,percent=$pct,track=$currentTrack,total=$totalTracks,message=Burning track $currentTrack of $totalTracks: $($file.Name)"
        
        $stream = New-Object -ComObject ADODB.Stream
        $stream.Type = 1 # adTypeBinary
        $stream.Open()
        $stream.LoadFromFile($file.FullName)
        
        $tao.AddAudioTrack($stream)
        $stream.Close()
    }
    
    Write-Output "PROGRESS:stage=Closing,percent=95.0,message=Finalizing audio disc"
    $tao.ReleaseMedia()
    
    if ($DoEject) {
        $rec.EjectMedia()
    }
    Write-Output "PROGRESS:stage=Finished,percent=100.0,message=Burn complete"
}

function Burn-Data {
    param(
        [string]$TargetId,
        [string]$Source,
        [int]$WriteSpeed,
        [bool]$DoEject
    )
    $dm = Get-DiscMaster
    $idx = [int]$TargetId
    $id = $dm.Item($idx)
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($id)
    
    Write-Output "PROGRESS:stage=Preparing,percent=5.0,message=Creating filesystem image"
    $fsi = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
    $fsi.ChooseImageDefaults($rec)
    $fsi.FileSystemsToCreate = 3 # ISO9660 | Joliet
    $fsi.Root.AddTree($Source, $false)
    $resultImage = $fsi.CreateResultImage()
    $imageStream = $resultImage.ImageStream
    
    $dataWriter = New-Object -ComObject IMAPI2.MsftDiscFormat2Data
    $dataWriter.Recorder = $rec
    $dataWriter.ClientName = "SpotyBurn"
    
    Write-Output "PROGRESS:stage=Writing,percent=20.0,message=Burning data CD"
    $dataWriter.Write($imageStream)
    
    Write-Output "PROGRESS:stage=Closing,percent=95.0,message=Finalizing data disc"
    if ($DoEject) {
        $rec.EjectMedia()
    }
    Write-Output "PROGRESS:stage=Finished,percent=100.0,message=Burn complete"
}

function Eject-Drive {
    param([string]$TargetId)
    $dm = Get-DiscMaster
    $idx = [int]$TargetId
    $id = $dm.Item($idx)
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($id)
    $rec.EjectMedia()
    [PSCustomObject]@{ success = $true } | ConvertTo-Json -Compress
}

switch ($Action) {
    "list" { List-Drives }
    "status" { Get-DriveStatus -TargetId $DriveId }
    "burn-audio" { Burn-Audio -TargetId $DriveId -Source $SourcePath -WriteSpeed $Speed -DoEject $Eject }
    "burn-data" { Burn-Data -TargetId $DriveId -Source $SourcePath -WriteSpeed $Speed -DoEject $Eject }
    "eject" { Eject-Drive -TargetId $DriveId }
}
