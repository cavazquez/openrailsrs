// Read original ACT/stock/speed-post parameters from unmodified OR 1.6.1 DLLs.
// Compile together with HeadlessCapture.cs, selecting /main:ActivityParameters.
using System;
using System.IO;
using System.Linq;
using System.Reflection;
using Newtonsoft.Json;
using Orts.Simulation;
using Orts.Simulation.RollingStocks;
using Orts.Formats.Msts;
internal static class ActivityParameters {
 static float ForwardHeading(Orts.Simulation.Signalling.SignalObject signal) { var t=new Orts.Simulation.Traveller(signal.tdbtraveller); if(t.Direction==Orts.Simulation.Traveller.TravellerDirection.Backward) t.ReverseDirection(); return t.RotY; }
 static int Main(string[] args) {
  var load=typeof(HeadlessCapture).GetMethod("Load",BindingFlags.Static|BindingFlags.NonPublic);
  var sim=(Simulator)load.Invoke(null,new object[]{args[0],null,true});
  var train=sim.PlayerLocomotive.Train;
  var traffic=sim.Activity.Tr_Activity.Tr_Activity_File.Player_Service_Definition.Player_Traffic_Definition;
  var locos=train.Cars.OfType<MSTSLocomotive>().ToArray();
  File.WriteAllText(args[1],JsonConvert.SerializeObject(new {
   scope="Original ACT initial native DLL parameters; no complete trajectory parity claim",
   activity=Path.GetFileName(args[0]),duration_estimate_s=sim.Activity.Tr_Activity.Tr_Activity_Header.Duration.ActivityDuration(),
   start_time_s=traffic.Time,cars=train.Cars.Count,length_m=train.Length,
   mass_kg=train.Cars.Sum(x=>(double)x.MassKG),max_power_w=locos.Sum(x=>(double)x.MaxPowerW),
   max_force_n=locos.Sum(x=>(double)x.MaxForceN),max_brake_n=train.Cars.Sum(x=>(double)x.MaxBrakeForceN),
   initial_allowed_speed_mps=train.AllowedMaxSpeedMpS,front_direction=train.FrontTDBTraveller.Direction.ToString(),front_node=train.FrontTDBTraveller.TrackNodeIndex,front_offset_m=train.FrontTDBTraveller.TrackNodeOffset,rear_offset_m=train.RearTDBTraveller.TrackNodeOffset,
   vehicles=train.Cars.Select(x=>new {source=Path.GetFileName(x.WagFilePath),mass_kg=x.MassKG,length_m=x.CarLengthM,max_brake_n=x.MaxBrakeForceN,brake_type=x.BrakeSystem.GetType().Name}),
   brake_controllers=locos.Select(x=>new {charged_psi=x.TrainBrakeController.MaxPressurePSI,full_service_drop_psi=x.TrainBrakeController.FullServReductionPSI}),
   timetable=traffic.Player_Traffic_List.Select(x=>new {platform=x.PlatformStartID,arrive_s=x.ArrivalTime.TimeOfDay.TotalSeconds-traffic.Time,depart_s=x.DepartTime.TimeOfDay.TotalSeconds-traffic.Time}),
   speed_posts=sim.TDB.TrackDB.TrItemTable.OfType<SpeedPostItem>().Where(x=>x.IsLimit&&!x.IsResume&&x.SigObj>=0).Select(x=>new {id=x.TrItemId,node=sim.Signals.SignalObjects[x.SigObj].trackNode,direction=sim.Signals.SignalObjects[x.SigObj].direction,traveller_direction=sim.Signals.SignalObjects[x.SigObj].tdbtraveller.Direction.ToString(),tc_direction=sim.Signals.SignalObjects[x.SigObj].TCDirection,offset_m=x.SData1,angle_rad=x.Angle,forward_heading_rad=ForwardHeading(sim.Signals.SignalObjects[x.SigObj]),speed_mps=x.IsMPH?x.SpeedInd*0.44704:x.SpeedInd/3.6,passenger=x.IsPassenger,freight=x.IsFreight})
  },Formatting.Indented).Replace("\r\n","\n")+"\n");return 0;
 }
}
