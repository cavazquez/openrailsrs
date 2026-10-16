// Full original activity trajectory, using the unmodified OR 1.6.1 DLLs.
// Compile beside HeadlessCapture.cs; its existing loader remains immutable.
using System;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using Newtonsoft.Json;
using ORTS.Common;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems;

internal static class ActivityTrajectory {
    static readonly CultureInfo C = CultureInfo.InvariantCulture;
    static object Scalars(object value, int nested) {
        var result = new System.Collections.Generic.SortedDictionary<string, object>();
        if (value == null) return result;
        for (Type type = value.GetType(); type != null && type != typeof(object); type = type.BaseType) {
            foreach (var field in type.GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly)) {
                object raw = field.GetValue(value);
                if (raw == null) continue;
                Type fieldType = raw.GetType();
                string key = type.Name + "." + field.Name;
                if (fieldType.IsPrimitive || fieldType.IsEnum || fieldType == typeof(decimal)) result[key] = raw;
                else if (nested > 0 && (field.Name.IndexOf("script", StringComparison.OrdinalIgnoreCase) >= 0 || field.Name.IndexOf("controller", StringComparison.OrdinalIgnoreCase) >= 0))
                    result[key] = Scalars(raw, nested - 1);
            }
        }
        return result;
    }
    static double TrainFrame(TrainCar car) {
        // OR 1.6.1 Train.UpdatePhysics, Train.cs2058--2066: motive and
        // gravity forces are local until this orientation is applied.
        bool rearCab = car.IsDriveable && car.Train.IsActualPlayerTrain && ((MSTSLocomotive)car).UsingRearCab;
        return car.Flipped ^ rearCab ? -1 : 1;
    }
    static string Header(Train train) {
        return "time_s,velocity_mps,odometer_m,throttle,brake,doors_open,brake_pipe_psi,cylinder_psi,motive_force_n,brake_force_n,friction_force_n,gravity_force_n,allowed_speed_mps,requested_throttle,requested_brake,motive_local_sum_n,gravity_local_sum_n"
            + String.Concat(train.Cars.Select((car, i) => ",car_"+i+"_motive_local_n,car_"+i+"_motive_train_n,car_"+i+"_gravity_local_n,car_"+i+"_gravity_train_n,car_"+i+"_train_frame_sign"));
    }
    static Train.StationStop Stop(Train train, ActivityTaskPassengerStopAt task) {
        // ActivityTask.IsAtStation intentionally tests StationStops[0], rather
        // than the task's own platform. The first stop remains there until the
        // departure event, so using it for the next task can invent an arrival.
        var stop = train.StationStops.FirstOrDefault(s => s.PlatformReference == task.PlatformEnd1.TrItemId || s.PlatformReference == task.PlatformEnd2.TrItemId);
        if (stop == null) throw new InvalidOperationException("Original station stop missing: " + task.PlatformEnd1.TrItemId);
        return stop;
    }
    static void Sample(StreamWriter writer, Simulator sim, double time, double throttle, double handle, bool doors) {
        var train = sim.PlayerLocomotive.Train;
        var loco = (MSTSLocomotive)sim.PlayerLocomotive;
        var values = new System.Collections.Generic.List<double> { time, Math.Abs(train.SpeedMpS), train.DistanceTravelledM,
            loco.ThrottleController.CurrentValue, loco.TrainBrakeController.CurrentValue, doors ? 1 : 0,
            loco.BrakeSystem.BrakeLine1PressurePSI, loco.BrakeSystem.GetCylPressurePSI(),
            train.Cars.Sum(c => TrainFrame(c) * c.MotiveForceN), train.Cars.Sum(c => (double)c.BrakeForceN),
            train.Cars.Sum(c => (double)c.FrictionForceN), train.Cars.Sum(c => TrainFrame(c) * c.GravityForceN), train.AllowedMaxSpeedMpS,
            throttle, handle, train.Cars.Sum(c => (double)c.MotiveForceN), train.Cars.Sum(c => (double)c.GravityForceN) };
        foreach (var car in train.Cars) {
            double sign = TrainFrame(car);
            values.AddRange(new double[] {car.MotiveForceN, sign * car.MotiveForceN, car.GravityForceN, sign * car.GravityForceN, sign});
        }
        writer.WriteLine(String.Join(",", values.Select(v => v.ToString("R", C))));
    }
    static int Main(string[] args) {
        try { return Run(args); }
        catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
    static int Run(string[] args) {
        var sim = (Simulator)typeof(HeadlessCapture).GetMethod("Load", BindingFlags.Static | BindingFlags.NonPublic).Invoke(null, new object[] { args[0], null, true });
        var loco = (MSTSLocomotive)sim.PlayerLocomotive;
        var tasks = sim.ActivityRun.Tasks.OfType<ActivityTaskPassengerStopAt>().ToArray();
        if (tasks.Length < 2) throw new InvalidDataException("Requires an original passenger activity with multiple stops");
        Directory.CreateDirectory(args[1]);
        double dt = 0.05, start = sim.ClockTime;
        var initial = new { cars = loco.Train.Cars.Count, mass_kg = loco.Train.Cars.Sum(c => (double)c.MassKG), length_m = loco.Train.Length,
            front_node = loco.Train.FrontTDBTraveller.TrackNodeIndex, front_offset_m = loco.Train.FrontTDBTraveller.TrackNodeOffset,
            front_direction = loco.Train.FrontTDBTraveller.Direction.ToString(), settings = sim.Settings,
            vehicles = loco.Train.Cars.Select(c => new { source = Path.GetFileName(c.WagFilePath), mass_kg = c.MassKG, length_m = c.CarLengthM, max_brake_n = c.MaxBrakeForceN,
                flipped = c.Flipped, train_frame_sign = TrainFrame(c) }) };
        // Settings is deliberately omitted from the public initial file: use
        // the fixed loader settings, rather than serializing host preferences.
        File.WriteAllText(Path.Combine(args[1], "initial.json"), JsonConvert.SerializeObject(new { initial.cars, initial.mass_kg, initial.length_m, initial.front_node, initial.front_offset_m, initial.front_direction, initial.vehicles,
            initialized_pneumatics = new {
                train = Scalars(loco.Train, 0), locomotive = Scalars(loco, 0),
                controller_actual = loco.TrainBrakeController.CurrentValue,
                throttle_actual = loco.ThrottleController.CurrentValue,
                controller = Scalars(loco.TrainBrakeController, 2),
                cylinders = loco.Train.Cars.Select(c => new {
                    source = Path.GetFileName(c.WagFilePath), type = c.BrakeSystem.GetType().FullName,
                    pipe_psi = c.BrakeSystem.BrakeLine1PressurePSI, cylinder_psi = c.BrakeSystem.GetCylPressurePSI(),
                    scalars = Scalars(c.BrakeSystem, 0) }) },
            station_stops = loco.Train.StationStops.Select(s => new { s.PlatformReference, s.SubrouteIndex, s.TCSectionIndex, s.Direction, s.StopOffset, s.ArrivalTime, s.DepartTime }) }, Formatting.Indented));
        Exception failure = null;
        var thread = new Thread(delegate() {
            try {
                int station = 0; bool boarding = false, doors = false; double boarded = 0;
                double throttle = 0, brake = 0;
                var arrivals = new System.Collections.Generic.List<object>();
                using (var writer = new StreamWriter(Path.Combine(args[1], "trace.csv"))) {
                    writer.WriteLine(Header(loco.Train));
                    Sample(writer, sim, 0, 0, 0, false);
                    for (int tick = 0; tick < 72000 && station < tasks.Length; tick++) {
                        double t = tick * dt, speed = Math.Abs(loco.Train.SpeedMpS);
                        var task = tasks[station];
                        var stop = Stop(loco.Train, task);
                        double distance = loco.Train.ComputeDistanceToNextStation(stop);
                        if (!boarding && speed <= 0.1 && loco.Train.CheckStationPosition(stop.PlatformItem, stop.Direction, stop.TCSectionIndex)) {
                            boarding = true; doors = true; boarded = 0;
                            loco.Train.SetDoors(DoorSide.Both, true);
                            arrivals.Add(new { platform = task.PlatformEnd1.TrItemId, arrival_s = t, position_error_m = distance, speed_mps = speed });
                        }
                        if (boarding) {
                            throttle = 0; brake = 0.69;
                            if (loco.Train.DoorState(DoorSide.Both) == DoorState.Open) boarded += dt;
                            double departure = task.SchDepart.TimeOfDay.TotalSeconds - start;
                            if (t >= departure && sim.ClockTime >= task.BoardingEndS && boarded >= 2) {
                                loco.Train.SetDoors(DoorSide.Both, false); doors = false;
                                if (loco.Train.DoorState(DoorSide.Both) == DoorState.Closed) { station++; boarding = false; }
                            }
                        } else {
                            if (distance < 0) throw new InvalidOperationException("Native platform is behind the train: " + task.PlatformEnd1.TrItemId);
                            double cap = Math.Min(loco.Train.AllowedMaxSpeedMpS * 0.9, 70 * 0.44704);
                            cap = Math.Min(cap, Math.Sqrt(2 * 0.28 * Math.Max(0, distance - 7)));
                            if (loco.Train.GetNextSignalAspect(0) == MstsSignalAspect.STOP && loco.Train.DistanceToSignal.HasValue)
                                cap = Math.Min(cap, Math.Sqrt(2 * 0.28 * Math.Max(0, loco.Train.DistanceToSignal.Value - 5)));
                            throttle = speed < cap - 0.5 ? 0.5 : 0;
                            brake = speed > cap + 0.2 ? (cap < 0.5 ? 0.69 : 0.45) : 0;
                        }
                        loco.SetDirection(Direction.Forward);
                        loco.SetThrottlePercentWithSound((float)(throttle * 100));
                        loco.SetTrainBrakePercent((float)(brake * 100));
                        sim.Update((float)dt);
                        Sample(writer, sim, (tick + 1) * dt, throttle, brake, doors);
                        if (tick % 6000 == 0) Console.Error.WriteLine("t={0:F2} station={1}/{2} distance={3:F2} speed={4:F3} odometer={5:F2}", t, station, tasks.Length, distance, loco.Train.SpeedMpS, loco.Train.DistanceTravelledM);
                        if (loco.Train.ControlMode == Train.TRAIN_CONTROL.OUT_OF_CONTROL)
                            throw new InvalidOperationException("Native train out of control: " + loco.Train.OutOfControlReason);
                    }
                }
                bool reached = station == tasks.Length;
                File.WriteAllText(Path.Combine(args[1], "outcome.json"), JsonConvert.SerializeObject(new { reached_all_platforms = reached,
                    original_task_count = tasks.Length, native_tasks_completed = tasks.Select(t => t.IsCompleted), elapsed_s = sim.ClockTime-start, distance_m = loco.Train.DistanceTravelledM, arrivals,
                    measurement = "throttle/brake are actual CurrentValue after original controllers; requested handles and every local/oriented vehicle force are retained; Train.UpdatePhysics Flipped XOR rear-cab orientation",
                    scope = "Original activity, timetable, stock, signals and brake handles; last terminal task may finish only when the train departs" }, Formatting.Indented));
                if (!reached) throw new InvalidOperationException("Native activity did not reach all stations within the declared 3600s");
            } catch (Exception error) { failure = error; }
        });
        thread.Name = "Updater Process"; thread.Start(); thread.Join();
        if (failure != null) throw new InvalidOperationException("Native full activity failed", failure);
        return 0;
    }
}
