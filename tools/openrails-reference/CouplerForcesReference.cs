// Only calls the unmodified Open Rails 1.6.1 parser, wagon getters and train
// force/impulse methods. No replacement coupler equations live in this helper.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Common;
using ORTS.Settings;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;

internal static class CouplerForcesReference
{
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj,string name,object value) {
        for(Type t=obj.GetType();t!=null;t=t.BaseType) {
            var p=t.GetProperty(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(p!=null) {p.SetValue(obj,value,null);return;}
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null) {f.SetValue(obj,value);return;}
        }
        throw new MissingMemberException(name);
    }
    static MSTSWagon Wagon(string text, bool flipped) {
        var car=Empty<MSTSWagon>();
        car.Couplers=new List<MSTSCoupling>();
        car.CouplerForceUSmoothed=new SmoothedData(1f);
        car.Flipped=flipped;
        car.Train=Empty<Train>(); car.Train.TrainType=Train.TRAINTYPE.STATIC;
        using(var stream=new MemoryStream(Encoding.UTF8.GetBytes(text)))
        using(var stf=new STFReader(stream,"coupler-force-owned-fixture",Encoding.UTF8,true)) {
            while(!stf.Eof) { stf.ReadItem(); car.Parse(stf.Tree.ToLowerInvariant(),stf); }
        }
        if(car.Couplers.Count==0) throw new InvalidDataException("Fixture did not parse any Coupling");
        return car;
    }
    static object Profile(string id, string text, bool flipped) {
        var car=Wagon(text,flipped);
        return new { id=id, flipped=flipped, count=car.Couplers.Count,
            advanced=car.IsAdvancedCoupler, rigid=car.GetCouplerRigidIndication(),
            zero_length_m=car.GetCouplerZeroLengthM(),
            simple_stiffness_npm=car.GetSimpleCouplerStiffnessNpM(),
            simple_slack_m=car.GetMaximumSimpleCouplerSlack1M(),
            simple_stroke_m=car.GetMaximumSimpleCouplerSlack2M(),
            warning_force_n=car.GetCouplerBreak1N(), break_force_n=car.GetCouplerBreak2N(),
            tension_zones_m=new[] {car.GetMaximumCouplerTensionSlack1M(),car.GetMaximumCouplerTensionSlack2M(),car.GetMaximumCouplerTensionSlack3M()},
            compression_zones_m=new[] {car.GetMaximumCouplerCompressionSlack1M(),car.GetMaximumCouplerCompressionSlack2M(),car.GetMaximumCouplerCompressionSlack3M()},
            tension_forces_n=new[] {car.GetCouplerTensionStiffness1N(),car.GetCouplerTensionStiffness2N()},
            compression_forces_n=new[] {car.GetCouplerCompressionStiffness1N(),car.GetCouplerCompressionStiffness2N()} };
    }
    static Train Pair(string text, JObject input) {
        var train=Empty<Train>(); train.TrainType=Train.TRAINTYPE.STATIC;
        var sim=Empty<Simulator>();Set(sim,"Settings",Empty<UserSettings>());Set(train,"Simulator",sim);
        var a=Wagon(text,false);var b=Wagon(text,false);
        Set(a,"Simulator",sim);Set(b,"Simulator",sim);
        a.MassKG=(float)input["leading_mass_kg"];b.MassKG=(float)input["trailing_mass_kg"];
        a.Train=train;b.Train=train;train.Cars=new List<TrainCar>{a,b};
        a.CouplerSlackM=(float)input["slack_m"];
        a.CouplerSlack2M=a.GetMaximumSimpleCouplerSlack2M();
        return train;
    }
    static object Force(string text,JObject input) {
        var train=Pair(text,input);var a=train.Cars[0];var b=train.Cars[1];
        a.SpeedMpS=b.SpeedMpS=1f;
        a.TotalForceN=(float)input["leading_force_n"];b.TotalForceN=(float)input["trailing_force_n"];
        train.ComputeCouplerForces(.05f);
        return new {id=(string)input["id"], native_coupler_force_u_n=a.CouplerForceU,
            tension_positive_force_n=-a.CouplerForceU,
            leading_total_force_n=a.TotalForceN,trailing_total_force_n=b.TotalForceN,
            slack_m=a.CouplerSlackM, dynamic_slack_limit_m=a.CouplerSlack2M};
    }
    static object Impact(string text,JObject input) {
        var train=Pair(text,input);var a=train.Cars[0];var b=train.Cars[1];
        a.SpeedMpS=(float)input["leading_velocity_mps"];b.SpeedMpS=(float)input["trailing_velocity_mps"];
        train.AddCouplerImpulseForces(.05f);
        return new {id=(string)input["id"],leading_velocity_mps=a.SpeedMpS,trailing_velocity_mps=b.SpeedMpS,
            native_impulse_u_ns=a.ImpulseCouplerForceUN,tension_positive_impulse_ns=-a.ImpulseCouplerForceUN};
    }
    static void Run(string[] args) {
        if(args.Length!=2)throw new ArgumentException("CouplerForcesReference INPUT.json OUTPUT.json");
        var input=JObject.Parse(File.ReadAllText(args[0]));
        var profiles=(JArray)input["profiles"];
        string simple=(string)profiles[0]["text"];
        var output=new {reference="Open Rails 1.6.1 unmodified assemblies",
            scope="Native STF coupler normalization and prescribed two-body ComputeCouplerForces/AddCouplerImpulseForces; not an OR driven journey or Rust hysteresis/energy reference",
            profiles=profiles.Select(row=>Profile((string)row["id"],(string)row["text"],(bool?)row["flipped"]??false)).ToArray(),
            forces=((JArray)input["forces"]).Select(row=>Force(simple,(JObject)row)).ToArray(),
            impacts=((JArray)input["impacts"]).Select(row=>Impact(simple,(JObject)row)).ToArray()};
        File.WriteAllText(args[1],JsonConvert.SerializeObject(output,Formatting.Indented)+"\n");
    }
    static void Main(string[] args) {
        try {Run(args);}
        catch(Exception error) {Console.Error.WriteLine(error.ToString());Environment.ExitCode=1;}
    }
}
