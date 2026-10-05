// Deterministic controls against the unmodified pinned Open Rails assemblies.
// This client captures a second formation; it never substitutes a physics DLL.
using System;
using System.IO;
using System.Linq;
using System.Threading;
using System.Globalization;
using System.Diagnostics;
using Newtonsoft.Json;
using ORTS.Settings;
using ORTS.Common;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.Physics;

class FormationCapture
{
    static readonly CultureInfo Culture = CultureInfo.InvariantCulture;
    static object Point(Traveller t) {
        var w = t.WorldLocation;
        return new { tile_x=w.TileX, tile_z=w.TileZ, x=w.Location.X,
            y=w.Location.Y, z=w.Location.Z, node=t.TrackNodeIndex,
            offset_m=t.TrackNodeOffset, direction=t.Direction.ToString() };
    }
    static string N(object n) {
        return n is float ? ((float)n).ToString("R",Culture) :
            n is double ? ((double)n).ToString("R",Culture) : Convert.ToString(n,Culture);
    }
    static void Input(MSTSLocomotive loco, double throttle, double service) {
        loco.SetDirection(Direction.Forward);
        loco.SetThrottlePercentWithSound((float)(100*throttle));
        // Demo Model 1's Davies & Metcalfe handle: Running=.1,
        // minimum reduction=.2 demand, smooth service=.25..8.
        float handle = service <= 0 ? .1f : (float)(.25 + .55 * Math.Max(0,(service-.2)/.8));
        loco.SetTrainBrakePercent(handle*100);
    }
    static void Sample(StreamWriter w, Simulator sim, double t, double throttle, double brake) {
        var loco=(MSTSDieselLocomotive)sim.PlayerLocomotive;var train=loco.Train;
        var head=train.FrontTDBTraveller;
        w.WriteLine(String.Join(",",new object[]{t,"e"+head.TrackNodeIndex+
            (head.Direction==Traveller.TravellerDirection.Backward?"_r":""),
            head.TrackNodeOffset,train.SpeedMpS,train.DistanceTravelledM,throttle,brake,
            loco.TrainBrakeController.CurrentValue,loco.BrakeSystem.BrakeLine1PressurePSI,
            loco.BrakeSystem.GetCylPressurePSI(),loco.DieselEngines[0].RealRPM,
            train.Cars.Sum(c=>(double)c.MotiveForceN),train.Cars.Sum(c=>(double)c.BrakeForceN),
            train.Cars.Sum(c=>(double)c.FrictionForceN),train.Cars.Sum(c=>(double)c.GravityForceN)}.Select(N)));
    }
    static void Run(Simulator sim, string output) {
        var loco=(MSTSDieselLocomotive)sim.PlayerLocomotive; var train=loco.Train;
        Input(loco,0,1);
        // Settle authored initial brakes before the common capture origin.
        for(int i=0;i<400;i++)sim.Update(.05f);
        File.WriteAllText(Path.Combine(output,"initial.json"),JsonConvert.SerializeObject(new {
            front=Point(train.FrontTDBTraveller),rear=Point(train.RearTDBTraveller),
            cars=train.Cars.Count,length_m=train.Length,mass_kg=train.Cars.Sum(c=>(double)c.MassKG),
            speed_mps=train.SpeedMpS,rpm=loco.DieselEngines[0].RealRPM,
            bearing_c=loco.WheelBearingTemperatureDegC,
            initial_odometer_m=train.DistanceTravelledM,
            vehicles=train.Cars.Select(c=>new {source=Path.GetFileName(c.WagFilePath),
                length_m=c.CarLengthM,mass_kg=c.MassKG,max_brake_force_n=c.MaxBrakeForceN,
                cylinder_psi=c.BrakeSystem.GetCylPressurePSI(),brake_type=c.BrakeSystem.GetType().Name}),
            adhesion_fields=typeof(MSTSLocomotive).GetFields(System.Reflection.BindingFlags.Instance |
                System.Reflection.BindingFlags.Public | System.Reflection.BindingFlags.NonPublic)
                .Where(f=>f.FieldType==typeof(float) && f.Name.ToLowerInvariant().Contains("adhesion"))
                .ToDictionary(f=>f.Name,f=>f.GetValue(loco))
        },Formatting.Indented));
        using(var writer=new StreamWriter(Path.Combine(output,"trace.csv"))) {
            writer.WriteLine("time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,throttle,brake,native_brake_handle,brake_pipe_psi,cylinder_psi,diesel_rpm,motive_force_n,brake_force_n,friction_force_n,gravity_force_n");
            Sample(writer,sim,0,0,1);
            for(int i=0;i<5000;i++) {
                double t=i*.05, throttle=t>=30&&t<110?.75:t>=180&&t<210?.5:0;
                double brake=t<10||t>=210?1:t>=150&&t<180?.6:0;
                Input(loco,throttle,brake);sim.Update(.05f);
                Sample(writer,sim,(i+1)*.05,throttle,brake);
            }
        }
        File.WriteAllText(Path.Combine(output,"outcome.json"),JsonConvert.SerializeObject(new {
            version="1.6.1",time_s=250,final_speed_mps=train.SpeedMpS,
            final_odometer_m=train.DistanceTravelledM,front=Point(train.FrontTDBTraveller),
            full_service=false,scope="Class 47: release, traction, coast, service braking, restart, full service"},Formatting.Indented));
    }
    static int Main(string[] a) {
        try {
            Thread.CurrentThread.CurrentCulture=Culture;Thread.CurrentThread.CurrentUICulture=Culture;
            Thread.CurrentThread.Name="Loader Process";
            Trace.Listeners.Add(new TextWriterTraceListener(Console.Error));Trace.AutoFlush=true;
            var settings=new UserSettings(new[]{"Language=en","UseAdvancedAdhesion=true",
                "SimpleControlPhysics=false","AdhesionFactor=100","AdhesionFactorChange=0",
                "AdhesionProportionalToWeather=false","GraduatedRelease=false",
                "BrakePipeChargingRate=21","CorrectQuestionableBrakingParams=false",
                "CurveSpeedDependent=false","TunnelResistanceDependent=false","WindResistanceDependent=false",
                "NoForcedRedAtStationStops=true","ActRandomizationLevel=0","ActWeatherRandomizationLevel=0",
                "BreakCouplers=false","Alerter=false","HotStart=true","ElectricHotStart=true"});
            var sim=new Simulator(settings,a[0],false,true);sim.Confirmer=new Confirmer(sim,1);
            sim.SetActivity(a[0]);sim.Start(new ORTS.Common.CancellationTokenSource(delegate{}).Token);
            sim.SetCommandReceivers();sim.PreUpdate=false;Directory.CreateDirectory(a[1]);
            Exception failure=null;var thread=new Thread(delegate(){try{Run(sim,a[1]);}catch(Exception e){failure=e;}});
            thread.Name="Updater Process";thread.Start();thread.Join();
            if(failure!=null)throw failure;
            return 0;
        }catch(Exception e){Console.Error.WriteLine(e);return 1;}
    }
}
