// Execute sound mathematics and SMS triggers from the pinned OR 1.6.1 binary.
// No simulator, OpenAL device, original content or UI state is created/modified.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Viewer3D;

class AudioReference
{
    class CountCommand : ORTSSoundCommand
    {
        public int Count;
        readonly ORTSSoundCommand inner;
        public CountCommand(SoundStream stream, ORTSSoundCommand inner) : base(stream)
        {
            this.inner = inner;
        }
        public override void Run() { Count++; inner.Run(); }
    }

    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Field(object obj, string name, object value)
    {
        obj.GetType().GetField(name, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic).SetValue(obj, value);
    }

    static void Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("AudioReference FIXTURE.sms OUTPUT.json");
        var sms = new SoundManagmentFile(args[0]);
        var group = sms.Tr_SMS.ScalabiltyGroups[0];
        var groups = new List<object>();
        foreach (var g in sms.Tr_SMS.ScalabiltyGroups)
            groups.Add(new { detail = g.DetailLevel, stereo = g.Stereo, ignore_3d = g.Ignore3D,
                activation_m = g.Activation.Distance, deactivation_m = g.Deactivation.Distance,
                cab = g.Activation.CabCam, passenger = g.Activation.PassengerCam, exterior = g.Activation.ExternalCam });

        var source = Empty<SoundSource>();
        var stream = Empty<SoundStream>();
        stream.SoundSource = source;
        stream.Volume = 1;
        var triggers = new List<ORTSVariableTrigger>();
        var commands = new List<CountCommand>();
        var thresholds = new List<float>();
        foreach (Variable_Trigger declaration in group.Streams[0].Triggers)
        {
            var trigger = Empty<ORTSVariableTrigger>();
            Field(trigger, "SMS", declaration);
            Field(trigger, "SoundStream", stream);
            trigger.Enabled = true;
            var command = new CountCommand(stream, ORTSSoundCommand.FromMSTS(declaration.SoundCommand, stream));
            trigger.SoundCommand = command;
            trigger.Initialize();
            triggers.Add(trigger);
            commands.Add(command);
            thresholds.Add(declaration.Threshold);
        }
        var checkpoints = new List<object>();
        foreach (float distance in new float[] { 50, 100, 101, 100, 99, 98, 101 })
        {
            source.DistanceSquared = distance * distance;
            foreach (var trigger in triggers) trigger.TryTrigger();
            checkpoints.Add(new { distance_m = distance, decrease_runs = commands[0].Count,
                increase_runs = commands[1].Count, volume = stream.Volume });
        }

        var attenuation = new List<object>();
        foreach (float maximum in new float[] { 0, 1000, 1500, 2000 })
        {
            Field(source, "DeactivationConditions", new Deactivation { Distance = maximum });
            typeof(SoundSource).GetMethod("SetRolloffFactor", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(source, null);
            foreach (float distance in new float[] { 0, 8, 25, 50, 100, 500, 1000, 1500, 2000 })
            {
                float clamped = Math.Max(SoundSource.ReferenceDistanceM, Math.Min(SoundSource.MaxDistanceM, distance));
                float gain = SoundSource.ReferenceDistanceM / (SoundSource.ReferenceDistanceM + source.RolloffFactor * (clamped - SoundSource.ReferenceDistanceM));
                attenuation.Add(new { deactivation_m = maximum, distance_m = distance, rolloff = source.RolloffFactor, gain = gain });
            }
        }
        var brakeEvents = new List<object>();
        foreach (int id in new int[] { 14, 54, 139, 141, 142, 143, 21, 22, 140 })
            brakeEvents.Add(new { id = id, native_event = Orts.Common.Events.From(Orts.Common.Events.Source.MSTSCar, id).ToString() });
        File.WriteAllText(args[1], JsonConvert.SerializeObject(new { reference = "Open Rails 1.6.1",
            groups = groups, distance_thresholds_squared = thresholds, distance_checkpoints = checkpoints,
            attenuation = attenuation, brake_events = brakeEvents }, Formatting.Indented).Replace("\r\n", "\n") + "\n");
    }
}
