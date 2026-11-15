# Commande vocale de Rewind : écoute « Rewind clip » (hors ligne, reconnaissance vocale de Windows).
# Écrit une ligne par événement : READY <langue>, CLIP <confiance> <texte>, HEARD <confiance> <texte>, ERR <message>.
# -Wav : lit un fichier au lieu du micro (tests automatiques).
# -Stdin : lit le son envoyé par Rewind (PCM 16 kHz mono 16 bits) : c'est le micro CHOISI dans Rewind,
#          pas le micro par défaut de Windows (qui peut être un micro virtuel, ex. Voicemod).
# Réglages mesurés le 2026-11-15 par l'entrée standard (voix de synthèse David / Zira, 3 vitesses) :
# commande seule ; « Rewind clip » 0,95 / 0,95 / 0,94 / 0,90 / 0,69 / rien ; « Remind me to clip it later » 0,72,
# les 9 autres phrases parasites rien. Seuil 0,75 → 10 parasites sur 10 refusées, 4 « Rewind clip » sur 6.
# Une dictée libre en concurrence (-DictationWeight) écarte bien les parasites mais rate la moitié des
# « Rewind clip » : pas utilisée. La durée de la phrase ne départage rien (1,1 s dans les deux cas).
# À régler avec une vraie voix : les phrases entendues sous le seuil sont notées dans le journal.
param([string]$Wav = "", [switch]$Stdin, [double]$MinConfidence = 0.75, [double]$MaxSeconds = 0, [double]$DictationWeight = 0, [int]$Rejection = -1, [switch]$Verbose)
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
    if ($Wav) {
        $engine.SetInputToWaveFile($Wav)
    } elseif ($Stdin) {
        # System.Speech refuse un flux qu'on ne peut pas « déplacer » (un tuyau) : petite enveloppe qui
        # se dit déplaçable et attend d'avoir reçu tout ce qui est demandé (une lecture courte = fin du flux)
        if (-not ("RewindPipe" -as [type])) {
            Add-Type -TypeDefinition @"
public class RewindPipe : System.IO.Stream {
    System.IO.Stream inner; long pos;
    public RewindPipe(System.IO.Stream s) { inner = s; }
    public override bool CanRead { get { return true; } }
    public override bool CanSeek { get { return true; } }
    public override bool CanWrite { get { return false; } }
    public override long Length { get { return long.MaxValue; } }
    public override long Position { get { return pos; } set { } }
    public override void Flush() { }
    public override long Seek(long offset, System.IO.SeekOrigin origin) { return pos; }
    public override void SetLength(long value) { }
    public override void Write(byte[] buffer, int offset, int count) { throw new System.NotSupportedException(); }
    public override int Read(byte[] buffer, int offset, int count) {
        int total = 0;
        while (total < count) {
            int n = inner.Read(buffer, offset + total, count - total);
            if (n <= 0) break;
            total += n;
        }
        pos += total;
        return total;
    }
}
"@
        }
        $fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
        $engine.SetInputToAudioStream((New-Object RewindPipe([Console]::OpenStandardInput())), $fmt)
    } else {
        $engine.SetInputToDefaultAudioDevice()
    }
    Say ("READY " + $info.Culture.Name)
    $empty = 0
    $fast = 0
    $clock = New-Object System.Diagnostics.Stopwatch
    while ($true) {
        $clock.Restart()
        try { $r = $engine.Recognize([TimeSpan]::FromSeconds(8)) } catch { if ($Wav -or $Stdin) { break } else { throw } }
        if ($r) {
            $empty = 0
            $fast = 0
            $len = 0
            try { $len = $r.Audio.Duration.TotalSeconds } catch {}
            if ($r.Grammar.Name -eq "rewind" -and $r.Confidence -ge $MinConfidence -and ($MaxSeconds -le 0 -or $len -le $MaxSeconds)) {
                Say ("CLIP {0:N2} {1:N2}s {2}" -f $r.Confidence, $len, $r.Text)
            } elseif ($Verbose) {
                Say ("HEARD {0:N2} {1:N2}s [{2}] {3}" -f $r.Confidence, $len, $r.Grammar.Name, $r.Text)
            }
        } elseif ($Wav) {
            $empty++
            if ($empty -ge 2) { break }
        } elseif ($Stdin) {
            # Flux fermé (Rewind arrêté) : Recognize rend la main tout de suite, sans attendre 8 s
            if ($clock.ElapsedMilliseconds -lt 200) { $fast++ } else { $fast = 0 }
            if ($fast -ge 5) { break }
        }
    }
} catch {
    Say ("ERR " + $_.Exception.Message)
    exit 1
}
