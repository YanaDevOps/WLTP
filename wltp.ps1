# WLTP - Portable Network Diagnostic Tool
# Modern WinMTR alternative for Windows
# Run: .\wltp.ps1 google.com

param(
    [Parameter(Mandatory=$false)]
    [string]$Target = "google.com",
    
    [Parameter(Mandatory=$false)]
    [int]$MaxHops = 30,
    
    [Parameter(Mandatory=$false)]
    [int]$TimeoutMs = 1000,
    
    [Parameter(Mandatory=$false)]
    [switch]$ExportHtml
)

# Function to interpret hop status
function Get-HopInterpretation {
    param(
        [PSCustomObject]$Hop,
        [bool]$IsDestination,
        [array]$NextHops
    )
    
    $status = "ok"
    $headline = ""
    $explanation = ""
    
    if ($Hop.Received -eq 0) {
        if ($IsDestination) {
            $status = "critical"
            $headline = "Destination not responding"
            $explanation = "The target server is not responding to ICMP requests. This could indicate the server is down, a firewall is blocking ICMP, or there's a network issue."
        } else {
            # Check if subsequent hops are responding
            $laterResponding = $NextHops | Where-Object { $_.Received -gt 0 -and $_.LossPercent -lt 10 }
            if ($laterResponding) {
                $status = "unknown"
                $headline = "Hop not responding (likely normal)"
                $explanation = "This router is not responding to ICMP but traffic is still reaching later hops. Many routers deprioritize or block ICMP responses."
            } else {
                $status = "critical"
                $headline = "Connection lost at this hop"
                $explanation = "Network connectivity is being lost at or before this hop. This suggests a real connectivity issue."
            }
        }
    } elseif ($Hop.LossPercent -gt 20) {
        $lossContinues = $NextHops | Where-Object { $_.LossPercent -gt 20 }
        if ($IsDestination) {
            $status = "critical"
            $headline = "$($Hop.LossPercent)% packet loss to destination"
            $explanation = "The target server is experiencing significant packet loss. This will affect application performance."
        } elseif ($lossContinues) {
            $status = "warning"
            $headline = "$($Hop.LossPercent)% packet loss starting here"
            $explanation = "Packet loss begins at this hop and continues to subsequent hops. This suggests a genuine network issue."
        } else {
            $status = "warning"
            $headline = "$($Hop.LossPercent)% loss (likely rate-limiting)"
            $explanation = "Packet loss at this hop, but subsequent hops are normal. Typically caused by ICMP rate limiting."
        }
    } elseif ($Hop.AvgLatency -gt 200) {
        if ($IsDestination) {
            $status = $Hop.AvgLatency -gt 500 ? "critical" : "warning"
            $headline = "High latency: $($Hop.AvgLatency)ms average"
            $explanation = "The destination server is responding with high latency. This will cause noticeable delays."
        } else {
            $status = "warning"
            $headline = "Elevated latency: $($Hop.AvgLatency)ms"
            $explanation = "This hop shows higher than optimal latency. May be ICMP deprioritization."
        }
    } elseif ($Hop.Jitter -gt 50) {
        $status = "warning"
        $headline = "High jitter: $($Hop.Jitter)ms variation"
        $explanation = "Significant latency variation. Can cause problems for real-time applications like VoIP and gaming."
    } else {
        $headline = "Healthy ($($Hop.AvgLatency)ms)"
        $explanation = "This hop is responding normally with acceptable latency and no significant packet loss."
    }
    
    return @{
        Status = $status
        Headline = $headline
        Explanation = $explanation
    }
}

# Function to get color for status
function Get-StatusColor {
    param([string]$Status)
    
    switch ($Status) {
        "ok"       { return "Green" }
        "warning"  { return "Yellow" }
        "critical" { return "Red" }
        default    { return "Gray" }
    }
}

# Function to get status symbol
function Get-StatusSymbol {
    param([string]$Status)
    
    switch ($Status) {
        "ok"       { return "✓" }
        "warning"  { return "⚠" }
        "critical" { return "✗" }
        default    { return "?" }
    }
}

# Main trace function
function Invoke-WLTPTrace {
    param([string]$Target)
    
    Write-Host "`n=== WLTP - Network Diagnostic Tool ===" -ForegroundColor Cyan
    Write-Host "Target: $Target`n" -ForegroundColor White
    
    # Run tracert
    Write-Host "Running trace route..." -ForegroundColor Yellow
    
    $tracertOutput = tracert -h $MaxHops -w $TimeoutMs $Target 2>&1
    $hops = @()
    $currentHop = $null
    
    foreach ($line in $tracertOutput) {
        $line = $line.Trim()
        
        # Skip empty lines and headers
        if ([string]::IsNullOrEmpty($line) -or 
            $line.StartsWith("Tracing") -or 
            $line.StartsWith("over a") -or
            $line.StartsWith("Trace complete")) {
            continue
        }
        
        # Parse hop line
        # Format: "1  <1 ms    <1 ms    <1 ms  192.168.1.1"
        if ($line -match "^(\d+)\s+(.*)$") {
            $hopIndex = [int]$Matches[1]
            $rest = $Matches[2]
            
            $hop = [PSCustomObject]@{
                Index = $hopIndex
                Hostname = $null
                IP = $null
                Sent = 0
                Received = 0
                LossPercent = 0
                Latencies = @()
                BestMs = $null
                WorstMs = $null
                AvgLatency = 0
                Jitter = 0
                Interpretation = $null
                Status = "unknown"
            }
            
            # Check for timeout
            $timeoutCount = ($rest | Select-String "\*").Matches.Count
            
            if ($timeoutCount -ge 3) {
                $hop.Sent = 3
                $hop.Received = 0
                $hop.LossPercent = 100
            } else {
                # Parse latencies and IP
                $parts = $rest -split '\s+'
                $ipFound = $false
                
                foreach ($part in $parts) {
                    if ([string]::IsNullOrEmpty($part)) { continue }
                    
                    # Try to parse as IP address
                    if ($part -match '^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$') {
                        if (-not $ipFound) {
                            $hop.IP = $part
                            $ipFound = $true
                        }
                    }
                    
                    # Try to parse as latency
                    if ($part -match '(\d+(?:<\d+)?)\s*ms') {
                        $latency = if ($Matches[1] -match '<(\d+)') {
                            [double]($Matches[1].Replace('<', '0').Replace('ms', ''))
                        } else {
                            [double]$Matches[1]
                        }
                        
                        if ($latency -gt 0) {
                            $hop.Sent++
                            $hop.Received++
                            $hop.Latencies += $latency
                        }
                    }
                }
                
                # Calculate statistics
                if ($hop.Latencies.Count -gt 0) {
                    $hop.BestMs = ($hop.Latencies | Measure-Object -Minimum).Minimum
                    $hop.WorstMs = ($hop.Latencies | Measure-Object -Maximum).Maximum
                    $hop.AvgLatency = ($hop.Latencies | Measure-Object -Average).Average
                    
                    # Calculate jitter (standard deviation)
                    $avg = $hop.AvgLatency
                    $variance = ($hop.Latencies | ForEach-Object {
                        [math]::Pow($_ - $avg, 2)
                    } | Measure-Object -Average).Average
                    $hop.Jitter = [math]::Sqrt($variance)
                    
                    $hop.LossPercent = 0
                }
            }
            
            $hops += $hop
        }
    }
    
    # Interpret each hop
    for ($i = 0; $i -lt $hops.Count; $i++) {
        $hop = $hops[$i]
        $isDestination = ($i -eq $hops.Count - 1)
        $nextHops = if ($i -lt $hops.Count - 1) { $hops[($i + 1)..($hops.Count - 1)] } else { @() }
        
        $interpretation = Get-HopInterpretation -Hop $hop -IsDestination $isDestination -NextHops $nextHops
        $hop.Interpretation = $interpretation
        $hop.Status = $interpretation.Status
    }
    
    # Generate summary
    $destination = $hops | Select-Object -Last 1
    $overallStatus = if ($destination.Received -eq 0) { "critical" } 
                     elseif ($destination.LossPercent -gt 20) { "critical" }
                     elseif ($destination.AvgLatency -gt 200) { "warning" }
                     else { "ok" }
    
    $primaryFinding = switch ($overallStatus) {
        "ok"       { "Connection looks stable" }
        "warning"  { "Some issues detected but connection is functional" }
        "critical" { "Significant connectivity problems detected" }
        default    { "Unable to determine status" }
    }
    
    $findings = @()
    $recommendations = @()
    
    if ($destination.Received -eq 0) {
        $findings += "Destination is not responding to ICMP requests"
        $recommendations += "Verify the destination address is correct"
        $recommendations += "The server may be down or blocking ICMP"
    } elseif ($destination.LossPercent -gt 20) {
        $findings += "High packet loss at destination: $($destination.LossPercent)%"
        $recommendations += "Contact your ISP or the destination server administrator"
    }
    
    if ($destination.AvgLatency -gt 200) {
        $findings += "Elevated latency to destination: $($destination.AvgLatency)ms"
    }
    
    if ($destination.Jitter -gt 50) {
        $findings += "High jitter at destination: $($destination.Jitter)ms"
        $recommendations += "For VoIP/gaming issues, check for bufferbloat on your router"
    }
    
    if ($recommendations.Count -eq 0) {
        $recommendations += "No action needed - connection is healthy"
    }
    
    # Display summary
    Write-Host "`n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    Write-Host "SUMMARY" -ForegroundColor White
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    
    $statusSymbol = Get-StatusSymbol -Status $overallStatus
    $statusColor = Get-StatusColor -Status $overallStatus
    
    Write-Host "`n  $statusSymbol $primaryFinding" -ForegroundColor $statusColor
    
    if ($findings.Count -gt 0) {
        Write-Host "`n  Observations:" -ForegroundColor White
        foreach ($finding in $findings) {
            Write-Host "    • $finding" -ForegroundColor Gray
        }
    }
    
    if ($recommendations.Count -gt 0) {
        Write-Host "`n  Recommended Actions:" -ForegroundColor White
        foreach ($rec in $recommendations) {
            Write-Host "    • $rec" -ForegroundColor Cyan
        }
    }
    
    # Display hops table
    Write-Host "`n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    Write-Host "NETWORK ROUTE" -ForegroundColor White
    Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor DarkGray
    
    Write-Host "`n  Status  Hop  Host                           Loss%    Sent  Recv  Best    Avg     Worst   Jitter   Interpretation" -ForegroundColor DarkGray
    Write-Host "  ──────  ────  ─────────────────────────────  ───────  ────  ────  ──────  ──────  ──────  ───────  ────────────────" -ForegroundColor DarkGray
    
    foreach ($hop in $hops) {
        $statusSymbol = Get-StatusSymbol -Status $hop.Status
        $statusColor = Get-StatusColor -Status $hop.Status
        
        $hostDisplay = if ($hop.Hostname) { $hop.Hostname } elseif ($hop.IP) { $hop.IP } else { "*" }
        $lossDisplay = "$($hop.LossPercent.ToString('F1'))%"
        $sentDisplay = "$($hop.Sent)"
        $recvDisplay = "$($hop.Received)"
        $bestDisplay = if ($hop.BestMs) { "$($hop.BestMs.ToString('F1'))" } else { "-" }
        $avgDisplay = if ($hop.AvgLatency) { "$($hop.AvgLatency.ToString('F1'))" } else { "-" }
        $worstDisplay = if ($hop.WorstMs) { "$($hop.WorstMs.ToString('F1'))" } else { "-" }
        $jitterDisplay = if ($hop.Jitter) { "$($hop.Jitter.ToString('F1'))" } else { "-" }
        
        Write-Host "  " -NoNewline
        Write-Host $statusSymbol -ForegroundColor $statusColor -NoNewline
        Write-Host "      " -NoNewline
        Write-Host ("{0,4}" -f $hop.Index) -NoNewline -ForegroundColor White
        Write-Host "  " -NoNewline
        Write-Host ("{0,-30}" -f $hostDisplay) -NoNewline -ForegroundColor White
        Write-Host "  " -NoNewline
        
        # Loss%
        if ($hop.LossPercent -gt 20) {
            Write-Host ("{0,6}" -f $lossDisplay) -NoNewline -ForegroundColor Red
        } else {
            Write-Host ("{0,6}" -f $lossDisplay) -NoNewline -ForegroundColor White
        }
        Write-Host "  " -NoNewline
        
        Write-Host ("{0,3}" -f $sentDisplay) -NoNewline -ForegroundColor Gray
        Write-Host "  " -NoNewline
        Write-Host ("{0,3}" -f $recvDisplay) -NoNewline -ForegroundColor Gray
        Write-Host "  " -NoNewline
        Write-Host ("{0,6}" -f $bestDisplay) -NoNewline -ForegroundColor Gray
        Write-Host " " -NoNewline
        Write-Host ("{0,6}" -f $avgDisplay) -NoNewline -ForegroundColor Gray
        Write-Host " " -NoNewline
        Write-Host ("{0,6}" -f $worstDisplay) -NoNewline -ForegroundColor Gray
        Write-Host " " -NoNewline
        Write-Host ("{0,6}" -f $jitterDisplay) -NoNewline -ForegroundColor Gray
        Write-Host "   " -NoNewline
        
        # Interpretation (truncated)
        $interp = $hop.Interpretation.Headline
        if ($interp.Length -gt 40) {
            $interp = $interp.Substring(0, 37) + "..."
        }
        Write-Host $interp -ForegroundColor $statusColor
    }
    
    Write-Host "`n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━`n" -ForegroundColor DarkGray
    
    # Export HTML if requested
    if ($ExportHtml) {
        $htmlPath = "wltp-report-$Target-$(Get-Date -Format 'yyyyMMdd-HHmmss').html"
        Export-HtmlReport -Hops $hops -Summary @{
            OverallStatus = $overallStatus
            PrimaryFinding = $primaryFinding
            Findings = $findings
            Recommendations = $recommendations
        } -Path $htmlPath
        Write-Host "HTML report saved to: $htmlPath`n" -ForegroundColor Green
    }
}

# Function to export HTML report
function Export-HtmlReport {
    param(
        [array]$Hops,
        [hashtable]$Summary,
        [string]$Path
    )
    
    $statusColor = switch ($Summary.OverallStatus) {
        "ok" { "#22c55e" }
        "warning" { "#eab308" }
        "critical" { "#ef4444" }
        default { "#6b7280" }
    }
    
    $statusText = switch ($Summary.OverallStatus) {
        "ok" { "Healthy" }
        "warning" { "Warning" }
        "critical" { "Critical" }
        default { "Unknown" }
    }
    
    $hopRows = ""
    foreach ($hop in $Hops) {
        $hopStatusColor = switch ($hop.Status) {
            "ok" { "#22c55e" }
            "warning" { "#eab308" }
            "critical" { "#ef4444" }
            default { "#6b7280" }
        }
        
        $ip = if ($hop.IP) { $hop.IP } else { "*" }
        $hostDisplay = if ($hop.Hostname) { $hop.Hostname } else { $ip }
        
        $loss = "$($hop.LossPercent.ToString('F1'))%"
        $best = if ($hop.BestMs) { "$($hop.BestMs.ToString('F1'))" } else { "-" }
        $avg = if ($hop.AvgLatency) { "$($hop.AvgLatency.ToString('F1'))" } else { "-" }
        $worst = if ($hop.WorstMs) { "$($hop.WorstMs.ToString('F1'))" } else { "-" }
        $jitter = if ($hop.Jitter) { "$($hop.Jitter.ToString('F1'))" } else { "-" }
        
        $headline = $hop.Interpretation.Headline -replace '"', '&quot;'
        $explanation = $hop.Interpretation.Explanation -replace '"', '&quot;'
        
        $hopRows += @"
            <tr>
                <td style="text-align: center; color: $hopStatusColor;">●</td>
                <td>$($hop.Index)</td>
                <td title="$ip">$hostDisplay</td>
                <td>$loss</td>
                <td>$($hop.Sent)</td>
                <td>$($hop.Received)</td>
                <td>$best</td>
                <td>$avg</td>
                <td>$worst</td>
                <td>$jitter</td>
                <td><strong>$headline</strong><br/><small style="color: #666;">$explanation</small></td>
            </tr>
"@
    }
    
    $findingsList = ($Summary.Findings | ForEach-Object { "<li>$_</li>" }) -join ""
    $recommendationsList = ($Summary.Recommendations | ForEach-Object { "<li>$_</li>" }) -join ""
    
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss UTC"
    
    $html = @"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WLTP Report - $Target</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; line-height: 1.6; color: #333; background: #f5f5f5; padding: 20px; }
        .container { max-width: 1200px; margin: 0 auto; background: white; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }
        .header { padding: 24px; border-bottom: 1px solid #eee; text-align: center; }
        .header h1 { margin: 0 0 10px; }
        .status-badge { display: inline-block; padding: 8px 16px; border-radius: 20px; color: white; font-weight: bold; margin: 10px 0; background: $statusColor; }
        .summary { padding: 24px; background: #fafafa; border-radius: 8px; margin: 20px; }
        .summary h2 { margin: 0 0 10px; }
        .findings, .recommendations { margin-top: 15px; }
        .findings h3, .recommendations h3 { font-size: 14px; color: #666; margin-bottom: 8px; }
        table { width: 100%; border-collapse: collapse; margin: 20px; }
        th, td { padding: 12px; text-align: left; border-bottom: 1px solid #eee; }
        th { background: #f9f9f9; font-weight: bold; }
        .footer { text-align: center; padding: 20px; color: #666; font-size: 12px; border-top: 1px solid #eee; margin-top: 20px; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>WLTP Network Diagnostic Report</h1>
            <div class="status-badge">$statusText</div>
            <div style="color: #666; margin-top: 10px;">Generated: $timestamp</div>
        </div>
        
        <div class="summary">
            <h2>Target: $Target</h2>
            <div style="font-size: 18px; font-weight: bold; margin-bottom: 10px; color: $statusColor;">
                $($Summary.PrimaryFinding)
            </div>
            
            <div class="findings">
                <h3>Observations</h3>
                <ul>$findingsList</ul>
            </div>
            
            <div class="recommendations">
                <h3>Recommended Actions</h3>
                <ul>$recommendationsList</ul>
            </div>
        </div>
        
        <div>
            <h2 style="font-size: 18px; margin: 20px;">Network Route</h2>
            <div style="overflow-x: auto;">
                <table>
                    <thead>
                        <tr style="text-xs text-gray-500">
                            <th style="width: 50px;">Status</th>
                            <th style="width: 50px;">Hop</th>
                            <th>Host</th>
                            <th style="width: 70px;">Loss%</th>
                            <th style="width: 50px;">Sent</th>
                            <th style="width: 50px;">Recv</th>
                            <th style="width: 70px;">Best</th>
                            <th style="width: 70px;">Avg</th>
                            <th style="width: 70px;">Worst</th>
                            <th style="width: 70px;">Jitter</th>
                            <th>Interpretation</th>
                        </tr>
                    </thead>
                    <tbody>
                        $hopRows
                    </tbody>
                </table>
            </div>
        </div>
        
        <div class="footer">
            Generated by WLTP - Modern WinMTR for Windows<br/>
            PowerShell Portable Version
        </div>
    </div>
</body>
</html>
"@
    
    $html | Out-File -FilePath $Path -Encoding UTF8
}

# Run the trace
try {
    Invoke-WLTPTrace -Target $Target
} catch {
    Write-Host "`nError: $_" -ForegroundColor Red
    Write-Host "`nUsage: .\wltp.ps1 [-Target hostname] [-MaxHops N] [-ExportHtml]" -ForegroundColor Yellow
    exit 1
}
