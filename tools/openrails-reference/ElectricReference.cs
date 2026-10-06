// Execute the unmodified OR 1.6.1 pantograph and default electric supply.
// Only subsystem inputs are supplied by this harness. No route, train, audio
// device or original installation is mutated.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.PowerSupplies;
using ORTS.Scripting.Api;

class ElectricReference
{
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for(Type t = obj.GetType(); t != null; t = t.BaseType) {
            var p = t.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if(p != null) { p.SetValue(obj, value, null); return; }
            var f = t.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if(f != null) { f.SetValue(obj, value); return; }
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static void Main(string[] args) {
        if(args.Length != 1) throw new ArgumentException("ElectricReference OUTPUT.json");
        var sim = Empty<Simulator>();
        sim.TRK = Empty<RouteFile>();
        sim.TRK.Tr_RouteFile = Empty<Tr_RouteFile>();
        sim.TRK.Tr_RouteFile.MaxLineVoltage = 25000;
        var loco = Empty<MSTSElectricLocomotive>();
        Set(loco, "Simulator", sim);
        loco.Pantographs = new Pantographs(loco);
        var panto = Empty<Pantograph>();
        Set(panto, "DelayS", 2f);
        Set(panto, "State", PantographState.Raising);
        loco.Pantographs.List.Add(panto);
        var pantographs = new List<object>();
        for(int i = 0; i < 4; i++) {
            panto.Update(.5f);
            pantographs.Add(new { elapsed_s = (i+1)*.5, fraction = panto.TimeS/panto.DelayS, state = panto.State.ToString(), command_up = panto.CommandUp });
        }
        Set(panto, "State", PantographState.Lowering);
        for(int i = 0; i < 4; i++) {
            panto.Update(.5f);
            pantographs.Add(new { elapsed_s = (i+5)*.5, fraction = panto.TimeS/panto.DelayS, state = panto.State.ToString(), command_up = panto.CommandUp });
        }
        var host = new ScriptedElectricPowerSupply(loco);
        loco.PowerSupply = host;
        Set(host, "PowerOnDelayS", 2f);
        Set(host, "AuxPowerOnDelayS", 0f);
        Set(host.ElectricTrainSupplySwitch, "Mode", ElectricTrainSupplySwitch.ModeType.Unfitted);
        var script = new DefaultElectricPowerSupply();
        Set(script, "Host", host);
        Set(script, "Car", loco);
        float clock = 0;
        script.GameTime = () => clock;
        script.ClockTime = () => clock;
        script.SignalEvent = evt => {};
        script.SignalEventToTrain = evt => {};
        script.Initialize();
        var checkpoints = new List<object>();
        float previous = 0;
        foreach(float t in new float[] { 0, .5f, 1, 1.5f, 2, 2.1f, 2.2f, 2.3f, 2.4f, 3.4f, 4.3f, 4.5f }) {
            clock = t;
            var ps = t == 2.2f ? PantographState.Raising : t == 2.3f ? PantographState.Down : PantographState.Up;
            var cb = t == 2.1f ? CircuitBreakerState.Open : CircuitBreakerState.Closed;
            Set(panto, "State", ps);
            Set(host.CircuitBreaker, "State", cb);
            script.Update(t - previous);
            checkpoints.Add(new { time_s = t, pantograph = ps.ToString(), breaker = cb.ToString(), main_power = host.MainPowerSupplyState == PowerSupplyState.PowerOn });
            previous = t;
        }
        File.WriteAllText(args[0], JsonConvert.SerializeObject(new {
            reference = "Open Rails 1.6.1", line_voltage_v = host.LineVoltageV,
            power_on_delay_s = host.PowerOnDelayS, pantograph_delay_s = panto.DelayS,
            pantographs = pantographs, checkpoints = checkpoints,
            scope = "Native pantograph movement and default main power state; no filtered-voltage or complete electric-locomotive parity claim."
        }, Formatting.Indented).Replace("\r\n", "\n") + "\n");
    }
}
