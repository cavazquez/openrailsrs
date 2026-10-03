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

        static float[] NativeSolarDirection(Viewer viewer)
        {
            var direction = (Vector3)typeof(SkyViewer).GetField("SolarDirection",
                BindingFlags.Instance | BindingFlags.NonPublic).GetValue(viewer.World.Sky);
            return new[] { direction.X, direction.Y, direction.Z };
        }

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
            // A remote target can change tiles after the initial activity load.
            // Wait for the original loader to publish the new WORLD window;
            // thirty rendered frames alone can still contain only old terrain.
            var scenery = viewer.World.Scenery.WorldFiles;
            if (!scenery.Any(tile => tile.TileX == viewer.Camera.TileX
                && tile.TileZ == viewer.Camera.TileZ))
            {
                readyFrames = 0;
                return;
            }
            if (++readyFrames == 30 && metadata != null)
            {
                var location = viewer.Camera.CameraWorldLocation;
                var orbit = viewer.Camera as ReferenceOrbitCamera;
                File.WriteAllText(metadata, JsonConvert.SerializeObject(new {
                    version = "1.6.1", view, time_s = viewer.Simulator.ClockTime,
                    tile_x = location.TileX, tile_z = location.TileZ,
                    location = location.Location, view_matrix = viewer.Camera.XnaView,
                    projection_matrix = viewer.Camera.XnaProjection,
                    solar_direction = NativeSolarDirection(viewer),
                    scenery_tiles = scenery.Select(tile => new[] { tile.TileX, tile.TileZ }).ToArray(),
                    orbit = orbit == null ? null : new {
                        yaw_rad = orbit.Yaw, pitch_rad = orbit.Pitch,
                        distance_m = orbit.Distance
                    },
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
        public readonly float Yaw, Pitch, Distance;

        static float ReadParameter(string name, float fallback)
        {
            float value;
            return Single.TryParse(Environment.GetEnvironmentVariable(name),
                NumberStyles.Float, CultureInfo.InvariantCulture, out value)
                && !Single.IsNaN(value) && !Single.IsInfinity(value) ? value : fallback;
        }

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
            Yaw = ReadParameter("OPENRAILS_REFERENCE_CAM_YAW", 1.6f);
            Pitch = ReadParameter("OPENRAILS_REFERENCE_CAM_PITCH", 0.6f);
            Distance = ReadParameter("OPENRAILS_REFERENCE_CAM_DIST", 160f);
            var offset = new Vector3(Distance * (float)Math.Cos(Pitch) * (float)Math.Sin(Yaw),
                Distance * (float)Math.Sin(Pitch), -Distance * (float)Math.Cos(Pitch) * (float)Math.Cos(Yaw));
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
