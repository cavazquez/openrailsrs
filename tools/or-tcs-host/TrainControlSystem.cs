// Explicit OR 1.6.1 API subset. Unsupported members fail compilation.
// API naming follows Source/Orts.Simulation/Common/Scripting/TrainControlSystem.cs.
namespace ORTS.Scripting.Api;

public enum TCSEvent { AlerterPressed, AlerterReleased, GenericTCSButtonPressed, GenericTCSButtonReleased }
public enum Aspect { Stop, Clear_2 }
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
    public Action<bool> SetEmergencyBrake = null!;
    public Action<bool> SetFullBrake = null!;
    public Action<float> SetCurrentSpeedLimitMpS = null!;
    public Action<float> SetNextSpeedLimitMpS = null!;
    public Action<float> SetInterventionSpeedLimitMpS = null!;
    public abstract void Initialize();
    public abstract void Update();
    public abstract void HandleEvent(TCSEvent evt, string message);
    // Host extensions are visibly named, rather than pretending to implement OR's full ETCSStatus.
    public Action<string, bool> HostMessage = null!;
    public Func<double> HostDeltaTimeS = null!;
    public Func<double?> HostNextStopDistanceM = null!;
}
