using ORTS.Scripting.Api;
public class MinimalTcs : TrainControlSystem
{
    bool acknowledged;
    bool restrict;
    public override void Initialize() { Activated = true; HostMessage("C# TCS listo: confirmar", true); }
    public override void Update()
    {
        var limit = restrict ? 5f : TrainSpeedLimitMpS();
        SetCurrentSpeedLimitMpS(limit);
        SetNextSpeedLimitMpS(limit);
        SetInterventionSpeedLimitMpS(limit + 1);
        SetEmergencyBrake(!acknowledged || SpeedMpS() > limit + 1);
    }
    public override void HandleEvent(TCSEvent evt, string message)
    {
        if (evt == TCSEvent.AlerterPressed) acknowledged = true;
        if (evt == TCSEvent.GenericTCSButtonPressed && message == "restrict") restrict = true;
    }
}
