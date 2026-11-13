# Simule une manche de Counter-Strike 2 en envoyant à Rewind les mêmes messages que le jeu (GSI).
# Rewind doit être lancé. Résultat attendu : un clip automatique « CS2 3K · Clutch · One Tap ».
param([int]$Port = 12762)

$token = (Get-Content "$env:LOCALAPPDATA\Rewind\cs2-token.txt" -Raw).Trim()
$me = "76561198000000001"

function Send-State($kills, $hs, $roundKills, $ammo, $phase = "live", $win = "") {
    $body = @{
        auth     = @{ token = $token }
        provider = @{ steamid = $me }
        map      = @{ name = "de_mirage"; mode = "competitive"; phase = "live"; round = 3 }
        round    = @{ phase = $phase; win_team = $win }
        player   = @{
            steamid     = $me; team = "T"
            state       = @{ health = 100; round_kills = $roundKills; round_killhs = $hs }
            match_stats = @{ kills = $kills; deaths = 0; assists = 0 }
            weapons     = @{ weapon_0 = @{ name = "weapon_ak47"; type = "Rifle"; state = "active"; ammo_clip = $ammo } }
        }
    } | ConvertTo-Json -Depth 6 -Compress
    Invoke-WebRequest -UseBasicParsing -Method Post -Uri "http://127.0.0.1:$Port/" -Body $body -ContentType "application/json" | Out-Null
}

Send-State 0 0 0 30
Start-Sleep -Milliseconds 800
Send-State 1 1 1 29          # une balle, headshot : one tap
Start-Sleep -Milliseconds 800
Send-State 2 1 2 25          # rafale : kill
Start-Sleep -Milliseconds 800
Send-State 3 2 3 22          # rafale : headshot
Start-Sleep -Milliseconds 300
Send-State 3 2 3 22 "over" "T"
"Manche simulée envoyée. Le clip arrive après quelques secondes (temps « après » de l'action)."
