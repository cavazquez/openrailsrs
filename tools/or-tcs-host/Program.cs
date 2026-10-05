using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Runtime.Loader;
using System.Text.Json;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;
using ORTS.Scripting.Api;

static class Program
{
    static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };
    static int Main(string[] args)
    {
        // Script Console.WriteLine must never contaminate the JSONL stream.
        using var protocol = new StreamWriter(Console.OpenStandardOutput()) { AutoFlush = true };
        Console.SetOut(TextWriter.Null);
        TrainControlSystem? script = null;
        Context context = new(0,0,0,0,null,false,null);
        Output output = new();
        long expected = 1;
        try
        {
            var file = args[Array.IndexOf(args, "--script") + 1];
            var typeName = args[Array.IndexOf(args, "--type") + 1];
            var sourceBytes = File.ReadAllBytes(file);
            var sourceHash = Convert.ToHexString(SHA256.HashData(sourceBytes)).ToLowerInvariant();
            var source = Encoding.UTF8.GetString(sourceBytes).TrimStart('\uFEFF');
            if (source.Length > 1024 * 1024) throw new InvalidDataException("Script exceeds 1 MiB");
            var references = ((string)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES")!).Split(Path.PathSeparator)
                .Append(typeof(TrainControlSystem).Assembly.Location).Distinct()
                .Select(p => MetadataReference.CreateFromFile(p));
            var compilation = CSharpCompilation.Create("SelectedTcs", [CSharpSyntaxTree.ParseText(source)], references,
                new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary));
            using var stream = new MemoryStream();
            var compiled = compilation.Emit(stream);
            if (!compiled.Success) throw new InvalidDataException(string.Join("; ", compiled.Diagnostics.Where(d => d.Severity == DiagnosticSeverity.Error)));
            stream.Position = 0;
            var assembly = AssemblyLoadContext.Default.LoadFromStream(stream);
            var type = assembly.GetType(typeName, throwOnError:true)!;
            if (!typeof(TrainControlSystem).IsAssignableFrom(type)) throw new InvalidDataException("Type must derive from ORTS.Scripting.Api.TrainControlSystem");
            script = (TrainControlSystem)Activator.CreateInstance(type)!;
            var persistent = type.GetMethod("Save")!.DeclaringType != typeof(TrainControlSystem)
                && type.GetMethod("Restore")!.DeclaringType != typeof(TrainControlSystem);
            script.ClockTime = () => (float)context.TimeS;
            script.SpeedMpS = () => (float)context.SpeedMps;
            script.TrainSpeedLimitMpS = () => (float)context.SpeedLimitMps;
            script.TrainMaxSpeedMpS = () => (float)(context.TrainMaxSpeedMps ?? context.SpeedLimitMps);
            script.CurrentSignalSpeedLimitMpS = () => context.NextSignalStop ? 0 : (float)context.SpeedLimitMps;
            script.NextSignalDistanceM = i => (float)(context.Signal(i)?.DistanceM ?? float.MaxValue);
            script.NextSignalAspect = i => NativeAspect(context.Signal(i)?.Aspect);
            script.NextDistanceSignalAspect = () => NativeAspect(context.DistanceSignal?.Aspect);
            script.NextDistanceSignalDistanceM = () => (float)(context.DistanceSignal?.DistanceM ?? float.MaxValue);
            script.CurrentPostSpeedLimitMpS = () => (float)(context.CurrentPostSpeedLimitMps ?? context.SpeedLimitMps);
            script.NextPostSpeedLimitMpS = i => (float)(context.Post(i)?.SpeedLimitMps ?? -1);
            script.NextPostDistanceM = i => (float)(context.Post(i)?.DistanceM ?? float.MaxValue);
            script.SetEmergencyBrake = b => output.EmergencyBrake = b;
            script.SetFullBrake = b => output.FullBrake = b;
            script.SetCurrentSpeedLimitMpS = v => output.AllowedMps = v;
            script.SetNextSpeedLimitMpS = v => output.NextLimitMps = v;
            script.SetInterventionSpeedLimitMpS = v => output.InterventionMps = v;
            script.HostDeltaTimeS = () => context.DtS;
            script.HostNextStopDistanceM = () => context.NextStopDistanceM;
            script.HostMessage = (text, ack) => {
                if (text.Length > 1024 || output.Messages.Count >= 32) throw new InvalidDataException("Message exceeds protocol limit");
                output.Messages.Add(new Message(text, ack, false));
            };
            while (ReadBoundedLine() is { } line)
            {
                var request = JsonSerializer.Deserialize<Request>(line, Json) ?? throw new InvalidDataException("Empty request");
                if (request.Version != 1 || request.Seq != expected) throw new InvalidDataException("Version/sequence mismatch");
                context = request.Context;
                SavedState? savedState = null;
                if (!context.Valid()) throw new InvalidDataException("Invalid SI context");
                if (request.Kind == "initialize" && expected == 1) {
                    output.AllowedMps = context.SpeedLimitMps;
                    output.InterventionMps = context.SpeedLimitMps + 2;
                    script.Initialize();
                } else if (request.Kind is "save" or "restore" && expected > 1) {
                    if (!persistent) throw new NotSupportedException("Script must override both Save and Restore");
                    if (request.Events.Length != 0) throw new InvalidDataException("Persistence must not consume pending inputs");
                    if (request.Kind == "save") {
                        using var data = new BoundedStateStream();
                        using var writer = new BinaryWriter(data, Encoding.UTF8, leaveOpen:true);
                        script.Save(writer); writer.Flush();
                        savedState = new SavedState(sourceHash, typeName, Convert.ToHexString(data.ToArray()).ToLowerInvariant(), script.Activated, output);
                    } else {
                        var state = request.State ?? throw new InvalidDataException("Missing script state");
                        if (state.SourceSha256 != sourceHash || state.TypeName != typeName || state.PayloadHex.Length > 16384)
                            throw new InvalidDataException("Incompatible script source/type or oversized state");
                        using var data = new MemoryStream(Convert.FromHexString(state.PayloadHex), writable:false);
                        using var reader = new BinaryReader(data, Encoding.UTF8, leaveOpen:true);
                        script.Restore(reader);
                        if (data.Position != data.Length) throw new InvalidDataException("Script did not consume its saved state");
                        if (!state.Output.Valid()) throw new InvalidDataException("Invalid saved outputs");
                        script.Activated = state.Activated;
                        output = state.Output;
                    }
                } else if (request.Kind != "tick" || expected == 1) throw new InvalidDataException("Invalid request kind");
                if (request.Events.Length > 64) throw new InvalidDataException("Too many inputs");
                foreach (var input in request.Events)
                {
                    if (input.Kind == "acknowledge") {
                        script.HandleEvent(TCSEvent.AlerterPressed, input.Message ?? "");
                        script.HandleEvent(TCSEvent.AlerterReleased, input.Message ?? "");
                        for (var i=0;i<output.Messages.Count;i++) if (output.Messages[i].Text == input.Message) output.Messages[i] = output.Messages[i] with { Acknowledged=true };
                    } else if (input.Kind == "menu") {
                        script.HandleEvent(TCSEvent.GenericTCSButtonPressed, input.Action ?? "");
                        script.HandleEvent(TCSEvent.GenericTCSButtonReleased, input.Action ?? "");
                    } else throw new InvalidDataException("Unknown input kind");
                }
                if (request.Kind is "initialize" or "tick") script.Update();
                if (!output.Valid()) throw new InvalidDataException("Output outside protocol limits");
                protocol.WriteLine(JsonSerializer.Serialize(new {version=1,seq=expected,status=output,error=(string?)null,persistent,state=savedState},Json));
                expected++;
            }
            return 0;
        }
        catch (Exception ex)
        {
            var message = (ex is TargetInvocationException ? ex.InnerException?.Message : ex.Message) ?? "C# host error";
            protocol.WriteLine(JsonSerializer.Serialize(new {version=1,seq=expected,status=(Output?)null,error=message[..Math.Min(message.Length,4096)]},Json));
            return 1;
        }
    }

    static string? ReadBoundedLine()
    {
        var line = new System.Text.StringBuilder();
        while (true) {
            var c = Console.In.Read();
            if (c < 0) return line.Length == 0 ? null : throw new EndOfStreamException("Truncated request");
            if (c == '\n') return line.ToString();
            if (line.Length >= 65536) throw new InvalidDataException("Request exceeds 64 KiB");
            line.Append((char)c);
        }
    }
    static Aspect NativeAspect(int? aspect) => aspect switch {
        null => Aspect.None, 0 => Aspect.Stop, 1 => Aspect.StopAndProceed, 2 => Aspect.Restricted,
        3 => Aspect.Approach_1, 4 => Aspect.Approach_2, 5 => Aspect.Approach_3,
        6 => Aspect.Clear_1, 7 => Aspect.Clear_2, _ => throw new InvalidDataException("Invalid native aspect")
    };
}
record Request(int Version, long Seq, string Kind, Context Context, Input[] Events, SavedState? State = null);
record SavedState(string SourceSha256, string TypeName, string PayloadHex, bool Activated, Output Output);
record Input(string Kind, string? Message, string? Action);
record Signal(double DistanceM, int Aspect);
record SpeedPost(double DistanceM, double SpeedLimitMps);
record Context(double TimeS, double DtS, double SpeedMps, double SpeedLimitMps, double? NextSignalDistanceM, bool NextSignalStop, double? NextStopDistanceM,
    double? TrainMaxSpeedMps = null, Signal[]? Signals = null, Signal? DistanceSignal = null, SpeedPost[]? SpeedPosts = null, double? CurrentPostSpeedLimitMps = null)
{
    static bool Speed(double v) => double.IsFinite(v) && v >= 0 && v <= 200;
    static bool Distance(double v) => double.IsFinite(v) && v >= 0;
    static bool Ordered(IEnumerable<double> distances) {
        double previous = -1;
        foreach (var distance in distances) { if (!Distance(distance) || distance < previous) return false; previous = distance; }
        return true;
    }
    public Signal? Signal(int index) {
        CheckIndex(index);
        if (Signals is { } signals) return index < signals.Length ? signals[index] : null;
        // Optional v1 fields preserve existing clients; absent signal != Clear.
        return index == 0 && NextSignalDistanceM is { } d ? new Signal(d, NextSignalStop ? 0 : 7) : null;
    }
    public SpeedPost? Post(int index) { CheckIndex(index); return SpeedPosts is { } posts && index < posts.Length ? posts[index] : null; }
    static void CheckIndex(int index) { if (index < 0 || index >= 32) throw new NotSupportedException("Lookahead index must be in 0..31"); }
    public bool Valid() => double.IsFinite(TimeS) && TimeS >= 0 && double.IsFinite(DtS) && DtS >= 0 && DtS <= 1
        && Speed(SpeedMps) && Speed(SpeedLimitMps) && (TrainMaxSpeedMps is null || Speed(TrainMaxSpeedMps.Value))
        && (CurrentPostSpeedLimitMps is null || Speed(CurrentPostSpeedLimitMps.Value))
        && (NextSignalDistanceM is null || Distance(NextSignalDistanceM.Value)) && (NextStopDistanceM is null || double.IsFinite(NextStopDistanceM.Value))
        && (Signals is null || Signals.Length <= 32 && Ordered(Signals.Select(s => s.DistanceM)) && Signals.All(s => s.Aspect >= 0 && s.Aspect <= 7))
        && (DistanceSignal is null || Distance(DistanceSignal.DistanceM) && DistanceSignal.Aspect >= 0 && DistanceSignal.Aspect <= 7)
        && (SpeedPosts is null || SpeedPosts.Length <= 32 && Ordered(SpeedPosts.Select(p => p.DistanceM)) && SpeedPosts.All(p => Speed(p.SpeedLimitMps)));
}
class Output {
    public double AllowedMps {get;set;}
    public double? NextLimitMps {get;set;}
    public double InterventionMps {get;set;}
    public bool EmergencyBrake {get;set;}
    public bool FullBrake {get;set;}
    public List<Message> Messages {get;set;} = [];
    public bool Valid() => new[] {AllowedMps, InterventionMps}.All(v => double.IsFinite(v) && v >= 0 && v <= 200)
        && (NextLimitMps is null || double.IsFinite(NextLimitMps.Value) && NextLimitMps >= 0 && NextLimitMps <= 200)
        && Messages is not null && Messages.Count <= 32 && Messages.All(m => m.Text is not null && Encoding.UTF8.GetByteCount(m.Text) <= 1024);
}
record Message(string Text, bool Acknowledgeable, bool Acknowledged);

// Cap writes while a script is serializing, before allocating a large buffer.
sealed class BoundedStateStream : MemoryStream {
    const int Limit = 8192;
    void Check(int count) { if (Position + count > Limit) throw new InvalidDataException("Script state exceeds 8 KiB"); }
    public override void Write(byte[] buffer, int offset, int count) { Check(count); base.Write(buffer, offset, count); }
    public override void Write(ReadOnlySpan<byte> buffer) { Check(buffer.Length); base.Write(buffer); }
    public override void WriteByte(byte value) { Check(1); base.WriteByte(value); }
    public override void SetLength(long value) { if (value > Limit) throw new InvalidDataException("Script state exceeds 8 KiB"); base.SetLength(value); }
}
