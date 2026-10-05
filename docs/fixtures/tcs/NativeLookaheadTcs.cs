using System;
using ORTS.Scripting.Api;

// Acceptance fixture, not a signalling/ETCS safety system.
public class NativeLookaheadTcs : TrainControlSystem
{
    public override void Initialize()
    {
        Activated = true;
        HostMessage("C# TCS nativo: " + NextSignalAspect(0), false);
    }
    public override void Update()
    {
        var aspect = NextSignalAspect(0);
        var allowed = Math.Min(TrainSpeedLimitMpS(), TrainMaxSpeedMpS());
        if (aspect == Aspect.Approach_1 || aspect == Aspect.Approach_2 || aspect == Aspect.Approach_3)
            allowed = Math.Min(allowed, CurrentPostSpeedLimitMpS() * 0.5f);
        SetCurrentSpeedLimitMpS(allowed);
        SetInterventionSpeedLimitMpS(allowed + 2);
        var next = NextPostSpeedLimitMpS(0);
        if (next >= 0) SetNextSpeedLimitMpS(next);
        // Deliberately conservative: the acceptance test checks physical brake
        // transfer. An operational system needs its own braking-distance curve.
        SetFullBrake(aspect == Aspect.Stop || aspect == Aspect.StopAndProceed);
    }
    public override void HandleEvent(TCSEvent evt, string message) { }
}
