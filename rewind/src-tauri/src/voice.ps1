# Commande vocale de Rewind : écoute « Rewind clip » (hors ligne, reconnaissance vocale de Windows).
# Écrit une ligne par événement : READY <langue>, CLIP <confiance> <texte>, HEARD <confiance> <texte>, ERR <message>.
# -Wav : lit un fichier au lieu du micro (tests automatiques).
# Réglages mesurés le 2026-11-13 (voix de synthèse David / Zira) : commande seule, seuil de rejet de
# Windows par défaut, confiance >= 0,70 → 8 phrases parasites sur 8 refusées, 3 « Rewind clip » sur 4 reconnus.
param([string]$Wav = "", [double]$MinConfidence = 0.70, [double]$DictationWeight = 0, [int]$Rejection = -1, [switch]$Verbose)
$ErrorActionPreference = "Stop"
function Say([string]$line) { [Console]::Out.WriteLine($line); [Console]::Out.Flush() }
try {
    Add-Type -AssemblyName System.Speech
    $all = [System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers()
    if ($all.Count -eq 0) { Say "ERR aucun moteur de reconnaissance vocale installe"; exit 2 }
    # « Rewind clip » est de l'anglais : moteur anglais de préférence
    $info = $all | Where-Object { $_.Culture.Name -like "en-*" } | Select-Object -First 1
    if (-not $info) { $info = $all[0] }
    $engine = New-Object System.Speech.Recognition.SpeechRecognitionEngine($info)

    $choices = New-Object System.Speech.Recognition.Choices
    # Une seule phrase : les variantes (« rewind clip that ») faisaient accepter « remind me to clip it »
    $choices.Add([string[]]@("rewind clip"))
    $builder = New-Object System.Speech.Recognition.GrammarBuilder
    $builder.Culture = $info.Culture
    $builder.Append($choices)
    $command = New-Object System.Speech.Recognition.Grammar($builder)
    $command.Name = "rewind"
    $command.Priority = 127
    $engine.LoadGrammar($command)
    # Dictée libre en concurrence, avec un poids faible : une phrase quelconque est reconnue comme
    # dictée (pas comme « rewind clip »), mais la commande garde l'avantage quand elle est prononcée
    if ($DictationWeight -gt 0) {
        $free = New-Object System.Speech.Recognition.DictationGrammar
        $free.Name = "libre"
        $free.Weight = [single]$DictationWeight
        $free.Priority = 0
        $engine.LoadGrammar($free)
    }

    if ($Rejection -ge 0) { try { $engine.UpdateRecognizerSetting("CFGConfidenceRejectionThreshold", [int]$Rejection) } catch {} }
    if ($Wav) { $engine.SetInputToWaveFile($Wav) } else { $engine.SetInputToDefaultAudioDevice() }
    Say ("READY " + $info.Culture.Name)
    $empty = 0
    while ($true) {
        try { $r = $engine.Recognize([TimeSpan]::FromSeconds(8)) } catch { if ($Wav) { break } else { throw } }
        if ($r) {
            $empty = 0
            if ($r.Grammar.Name -eq "rewind" -and $r.Confidence -ge $MinConfidence) {
                Say ("CLIP {0:N2} {1}" -f $r.Confidence, $r.Text)
            } elseif ($Verbose) {
                Say ("HEARD {0:N2} [{1}] {2}" -f $r.Confidence, $r.Grammar.Name, $r.Text)
            }
        } elseif ($Wav) {
            $empty++
            if ($empty -ge 2) { break }
        }
    }
} catch {
    Say ("ERR " + $_.Exception.Message)
    exit 1
}
