// Calls the unmodified OR 1.6.1 lateral-risk method. No replacement equations.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Microsoft.Xna.Framework;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Common;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;

internal static class BogieDynamicsReference
{
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for (Type t=obj.GetType(); t!=null; t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null) { f.SetValue(obj,value); return; }
        }
        throw new MissingFieldException(name);
    }
    static object Capture(JObject p, JObject c) {
        var train=Empty<Train>(); train.TrainType=Train.TRAINTYPE.PLAYER;
        train.WagonCoefficientFriction=(float)c["friction"];
        var car=Empty<MSTSWagon>(); car.Train=train;
        car.MassKG=(float)p["mass_kg"];
        Set(car,"WagonNumAxles",(int?)(c["axles_override"])??(int)p["axles"]);
        car.WagonNumBogies=(float?)(c["bogies_override"])??(float)p["bogies"];
        Set(car,"TrackGaugeM",(float)p["gauge_m"]);
        car.CentreOfGravityM=new Vector3(0,(float)p["cog_height_m"],0);
        Set(car,"RigidWheelBaseM",(float)p["rigid_wheelbase_m"]);
        car.MaximumWheelFlangeAngleRad=(float)p["flange_angle_rad"];
        car.WheelFlangeLengthM=(float)p["flange_height_m"];
        car.CurrentCurveRadiusM=(float)c["radius_m"];
        car.SuperElevationAngleRad=(float)c["cant_angle_rad"];
        car.SpeedMpS=(float)c["speed_mps"];
        // TrainCar.Update caches this immediately before the risk method.
        // Prescribed subsystems must set both fields, including reverse speed.
        car.AbsSpeedMpS=Math.Abs(car.SpeedMpS);
        car.DerailmentCoefficientEnabled=true;
        car.CouplerForceU=(bool)c["buff"]?1f:-1f;
        car.CouplerSlackM=(bool)c["buff"]?-.02f:.02f;
        car.WagonRearCouplerAngleRad=(float)c["coupler_angle_rad"];
        car.WagonRearCouplerBuffAngleRad=(float)c["coupler_angle_rad"];
        car.TotalWagonLateralDerailForceN=(float?)c["initial_lateral_n"]??0f;
        car.DerailExpected=(bool?)c["seed_flags"]??false;
        car.HasDerailed=(bool?)c["seed_flags"]??false;
        car.DerailPossible=(bool?)c["seed_flags"]??false;
        if((bool)c["ahead"]) {
            var ahead=Empty<MSTSWagon>(); ahead.Train=train;
            ahead.CouplerForceUSmoothed=new SmoothedData(1f);
            ahead.CouplerForceUSmoothed.ForceSmoothValue((float)c["ahead_force_n"]);
            car.CarAhead=ahead;
        }
        // No rear neighbour: prescribed coupler angles remain inputs to the
        // lateral branch; visual hose/coupler geometry is outside this fixture.
        car.CarBehind=null;
        car.UpdateTrainDerailmentRisk((float)c["dt_s"]);
        return new {profile=(string)p["id"],id=(string)c["id"],
            vertical_force_n=car.TotalWagonVerticalDerailForceN,
            lateral_force_n=car.TotalWagonLateralDerailForceN,
            derailment_coefficient=car.DerailmentCoefficient,
            nadal_coefficient=car.NadalDerailmentCoefficient,
            angle_of_attack_rad=car.AngleOfAttackRad,
            climb_distance_m=car.DerailClimbDistanceM,
            elapsed_s=car.DerailElapsedTimeS,possible=car.DerailPossible,
            expected=car.DerailExpected,has_derailed=car.HasDerailed,
            buff_force_exceeded=car.BuffForceExceeded};
    }
    static void Run(string[] args) {
        if(args.Length!=2) throw new ArgumentException("BogieDynamicsReference INPUT.json OUTPUT.json");
        var input=JObject.Parse(File.ReadAllText(args[0]));
        var rows=new List<object>();
        foreach(JObject p in (JArray)input["profiles"])
            foreach(JObject c in (JArray)input["cases"]) rows.Add(Capture(p,c));
        File.WriteAllText(args[1],JsonConvert.SerializeObject(new {
            reference="Open Rails 1.6.1 unmodified assemblies",
            scope=(string)input["scope"],rows=rows},Formatting.Indented)+"\n");
    }
    static void Main(string[] args) {
        try { Run(args); }
        catch(Exception error) { Console.Error.WriteLine(error.ToString()); Environment.ExitCode=1; }
    }
}
