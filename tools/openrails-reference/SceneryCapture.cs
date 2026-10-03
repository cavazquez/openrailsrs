// Client of the pinned, unmodified Open Rails 1.6.1 assemblies.
// Freeze the native viewer and select a reproducible exterior / 3D cab pose.
using System;
using System.IO;
using System.Globalization;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using Microsoft.Xna.Framework;
using Newtonsoft.Json;
using Orts.Viewer3D;
using Orts.Viewer3D.Processes;
using ORTS.Common;
using ORTS.Settings;
using Game = Orts.Viewer3D.Processes.Game;

class SceneryCapture
{
    [DllImport("kernel32.dll", CallingConvention = CallingConvention.StdCall)]
    static extern bool SetDllDirectory(string path);

    [STAThread]
    static void Main(string[] args)
    {
        SetDllDirectory(Path.Combine(ApplicationInfo.ProcessDirectory, "Native", Environment.Is64BitProcess ? "X64" : "X86"));
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        var settings = new UserSettings(args.Where(a => a.StartsWith("-")).Select(a => a.Substring(1)));
        using (var game = new CaptureGame(settings))
        {
            // Wine's PDH implementation lacks PdhFormatFromRawValue. Host only
            // samples memory / CPU statistics; stop it before its first sample.
            // Physics, content loading, camera and drawing remain native.
            game.HostProcess.Stop();
            typeof(Game).GetMethod("PushState", BindingFlags.Instance | BindingFlags.NonPublic)
                .Invoke(game, new object[] { new GameStateRunActivity(args) });
            game.Run();
        }
    }

    class CaptureGame : Game
    {
        bool configured;
        int readyFrames;
        readonly string view = Environment.GetEnvironmentVariable("OPENRAILS_REFERENCE_VIEW") ?? "exterior";
        readonly string metadata = Environment.GetEnvironmentVariable("OPENRAILS_REFERENCE_METADATA");

        public CaptureGame(UserSettings settings) : base(settings) { }

        protected override void Update(GameTime time)
        {
            base.Update(time);
            var program = typeof(Game).Assembly.GetType("Orts.Program");
            var viewer = (Viewer)program.GetField("Viewer", BindingFlags.Static | BindingFlags.Public).GetValue(null);
            if (viewer == null || viewer.Camera == null || viewer.QuitWindow == null)
                return;
            viewer.Simulator.Paused = true;
            viewer.QuitWindow.Visible = false;
            if (!configured)
            {
                if (view == "cab")
                    viewer.ThreeDimCabCamera.Activate();
                else
                    new ReferenceOrbitCamera(viewer).Activate();
                configured = true;
            }
            if (++readyFrames == 30 && metadata != null)
            {
                var location = viewer.Camera.CameraWorldLocation;
                File.WriteAllText(metadata, JsonConvert.SerializeObject(new {
                    version = "1.6.1", view, time_s = viewer.Simulator.ClockTime,
                    tile_x = location.TileX, tile_z = location.TileZ,
                    location = location.Location, view_matrix = viewer.Camera.XnaView,
                    projection_matrix = viewer.Camera.XnaProjection,
                    target_override = Environment.GetEnvironmentVariable("OPENRAILS_REFERENCE_TARGET"),
                    host_statistics_disabled = true
                }, Formatting.Indented));
                Console.WriteLine("Native scenery capture ready");
            }
        }
    }

    class ReferenceOrbitCamera : FreeRoamCamera
    {
        readonly Vector3 target;

        public ReferenceOrbitCamera(Viewer viewer) : base(viewer, viewer.Camera)
        {
            var center = viewer.Simulator.PlayerLocomotive.WorldPosition.WorldLocation;
            var targetOverride = Environment.GetEnvironmentVariable("OPENRAILS_REFERENCE_TARGET");
            if (!String.IsNullOrEmpty(targetOverride))
            {
                var values = targetOverride.Split(',');
                if (values.Length != 5)
                    throw new ArgumentException("OPENRAILS_REFERENCE_TARGET must be tileX,tileZ,localX,Y,localZ");
                center = new WorldLocation(
                    Int32.Parse(values[0], CultureInfo.InvariantCulture),
                    Int32.Parse(values[1], CultureInfo.InvariantCulture),
                    new Vector3(Single.Parse(values[2], CultureInfo.InvariantCulture),
                        Single.Parse(values[3], CultureInfo.InvariantCulture),
                        Single.Parse(values[4], CultureInfo.InvariantCulture)));
            }
            target = center.Location;
            const float yaw = 1.6f, pitch = 0.6f, distance = 160f;
            var offset = new Vector3(distance * (float)Math.Cos(pitch) * (float)Math.Sin(yaw),
                distance * (float)Math.Sin(pitch), -distance * (float)Math.Cos(pitch) * (float)Math.Cos(yaw));
            cameraLocation = new WorldLocation(center.TileX, center.TileZ, target + offset);
        }

        protected override Matrix GetCameraView()
        {
            var position = cameraLocation.Location;
            return Matrix.CreateLookAt(new Vector3(position.X, position.Y, -position.Z),
                new Vector3(target.X, target.Y, -target.Z), Vector3.Up);
        }
    }
}
