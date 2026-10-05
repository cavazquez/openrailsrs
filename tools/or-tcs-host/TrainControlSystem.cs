// Explicit OR 1.6.1 API subset. Unsupported members fail compilation.
// API naming follows Source/Orts.Simulation/Common/Scripting/TrainControlSystem.cs.
namespace ORTS.Scripting.Api;

public enum TCSEvent { AlerterPressed = 3, AlerterReleased = 4, GenericTCSButtonPressed = 14, GenericTCSButtonReleased = 15 }
// OR's API order differs from SIGASP (which starts with Stop = 0).
public enum Aspect { None, Clear_2, Clear_1, Approach_3, Approach_2, Approach_1, Restricted, StopAndProceed, Stop, Permission }
public abstract class TrainControlSystem
{
    public bool Activated { get; set; }
    public Func<float> ClockTime = null!;
    public Func<float> SpeedMpS = null!;
    public Func<float> TrainSpeedLimitMpS = null!;
    public Func<float> TrainMaxSpeedMpS = null!;
    public Func<float> CurrentSignalSpeedLimitMpS = null!;
    public Func<int, float> NextSignalDistanceM = null!;
    public Func<int, Aspect> NextSignalAspect = null!;
    public Func<Aspect> NextDistanceSignalAspect = null!;
    public Func<float> NextDistanceSignalDistanceM = null!;
    public Func<float> CurrentPostSpeedLimitMpS = null!;
    public Func<int, float> NextPostSpeedLimitMpS = null!;
    public Func<int, float> NextPostDistanceM = null!;
    public Action<bool> SetEmergencyBrake = null!;
    public Action<bool> SetFullBrake = null!;
    public Action<float> SetCurrentSpeedLimitMpS = null!;
    public Action<float> SetNextSpeedLimitMpS = null!;
    public Action<float> SetInterventionSpeedLimitMpS = null!;
    public abstract void Initialize();
    public abstract void Update();
    public virtual void Save(System.IO.BinaryWriter outf) { }
    public virtual void Restore(System.IO.BinaryReader inf) { }
    public abstract void HandleEvent(TCSEvent evt, string message);
    // Host extensions are visibly named, rather than pretending to implement OR's full ETCSStatus.
    public Action<string, bool> HostMessage = null!;
    public Func<double> HostDeltaTimeS = null!;
    public Func<double?> HostNextStopDistanceM = null!;
}
