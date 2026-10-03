// Calls the pinned, unmodified Open Rails 1.6.1 geometry and solar equations.
// No renderer, simulator loop or user Wine prefix is required.
using System;
using System.IO;
using System.Reflection;
using Microsoft.Xna.Framework;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Orts.Common;
using Orts.Formats.Msts;

class SolarReference
{
    static void Main(string[] args)
    {
        var assembly = Assembly.LoadFrom("RunActivity.exe");
        var dateType = assembly.GetType("Orts.Viewer3D.SkyViewer+SkyDate", true);
        var method = assembly.GetType("Orts.Viewer3D.Common.SunMoonPos", true).GetMethod("SolarAngle");
        var rows = new JArray();
        foreach (JObject sample in JArray.Parse(File.ReadAllText(args[0])))
        {
            double latitude = 0, longitude = 0;
            int status = new WorldLatLon().ConvertWTC((int)sample["tile_x"], (int)sample["tile_z"],
                new Vector3((float)sample["local_x"], 0, (float)sample["local_z"]), ref latitude, ref longitude);
            sample["status"] = status;
            if (status == 1)
            {
                sample["latitude"] = latitude;
                sample["longitude"] = longitude;
                int season = (int)sample["season_index"];
                int ordinal = latitude >= 0 ? 82 + season * 91 : (82 + (season + 2) * 91) % 365;
                sample["ordinal"] = ordinal;
                object date = Activator.CreateInstance(dateType);
                dateType.GetField("OrdinalDate").SetValue(date, ordinal);
                EnvironmentFile.SkySatellite sun = null;
                if (sample["environment_file"] != null)
                    sun = new EnvironmentFile(Path.Combine(args[2], "ENVFILES", (string)sample["environment_file"])).Sun;
                sample["rise_time_s"] = sun == null ? 0 : sun.RiseTime;
                sample["set_time_s"] = sun == null ? 0 : sun.SetTime;
                Vector3 direction = (Vector3)method.Invoke(null, new object[] {
                    latitude, longitude, sun, (float)((double)sample["time_s"] / 86400), date
                });
                sample["direction"] = JArray.FromObject(new[] { direction.X, direction.Y, direction.Z });
            }
            rows.Add(sample);
        }
        var result = new JObject();
        result["version"] = "1.6.1";
        result["commit"] = "d16e670da333d26d2edfc97d5631a19dadf49ce5";
        result["consumer"] = "tools/openrails-reference/SolarReference.cs";
        result["samples"] = rows;
        File.WriteAllText(args[1], result.ToString(Formatting.Indented));
    }
}
