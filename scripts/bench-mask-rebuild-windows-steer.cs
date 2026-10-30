// Pointer steering for scripts/bench-mask-rebuild-windows-flame.ps1 (#428),
// and the bench copy of a character that its -Scenario runs.
// Kept to C# 5 so Windows PowerShell 5.1's Add-Type compiles it.
//
// fidget writes a frame line to process.log every frame and keeps the file
// open. NTFS does not raise a change notification for those appends until the
// handle closes, so neither FileSystemWatcher nor Get-Content -Wait (which
// re-reads once a second) sees them in time. The script instead wakes on each
// display refresh (Refresh), reads what was appended (LogFollower), and puts
// the pointer on the newest frame's sprite centre (Screen).

using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

namespace FidgetBench
{
    public sealed class LogFollower : IDisposable
    {
        static readonly Regex Stamp = new Regex(@"^(\d+) ");
        static readonly Regex FrameLine = new Regex(@"^\d+ frame: (\d+) .* sprite\((-?\d+),(-?\d+)\) (\S+)#");

        readonly string path;
        readonly long since;
        readonly int halfWidth;
        readonly int halfHeight;
        readonly double scale;
        readonly byte[] chunk = new byte[64 * 1024];
        readonly List<byte> partial = new List<byte>();
        FileStream stream;

        public int AimX { get; private set; }
        public int AimY { get; private set; }
        public int Frames { get; private set; }
        public List<string> Rebuilds { get; private set; }
        // Per frame line: how long after fidget stamped it this follower read it.
        public List<long> LagsMs { get; private set; }
        // The animation a scenario times; null times every frame. A rebuild
        // counts when the newest frame line before it names this animation,
        // and OnScreenMs adds up the frame-stamp time it was drawn, as
        // scripts/measure-mask-rebuild-x11.sh does on X11.
        public string Steady { get; set; }
        public List<string> Counted { get; private set; }
        public long OnScreenMs { get { return onMs + (on ? lastFrameMs - onSinceMs : 0); } }

        bool on;
        long onSinceMs;
        long lastFrameMs;
        long onMs;

        // since: Unix seconds; earlier lines are skipped. halfWidth, halfHeight:
        // the sprite centre's offset from its top-left, in fidget's points.
        // scale: physical pixels per point on the primary display.
        public LogFollower(string path, long since, int halfWidth, int halfHeight, double scale)
        {
            this.path = path;
            this.since = since;
            this.halfWidth = halfWidth;
            this.halfHeight = halfHeight;
            this.scale = scale;
            Rebuilds = new List<string>();
            LagsMs = new List<long>();
            Counted = new List<string>();
        }

        public bool Step()
        {
            return Step(DateTimeOffset.UtcNow.ToUnixTimeMilliseconds());
        }

        // Reads everything appended since the last call. True when a new frame
        // line moved the aim; the aim is the newest one's sprite centre.
        public bool Step(long nowMs)
        {
            if (stream == null)
            {
                if (!File.Exists(path)) return false;
                stream = new FileStream(path, FileMode.Open, FileAccess.Read,
                    FileShare.ReadWrite | FileShare.Delete);
            }
            bool moved = false;
            int read;
            while ((read = stream.Read(chunk, 0, chunk.Length)) > 0)
            {
                for (int i = 0; i < read; i++)
                {
                    if (chunk[i] != (byte)'\n') { partial.Add(chunk[i]); continue; }
                    string line = Encoding.UTF8.GetString(partial.ToArray()).TrimEnd('\r');
                    partial.Clear();
                    moved |= Take(line, nowMs);
                }
            }
            return moved;
        }

        bool Take(string line, long nowMs)
        {
            Match stamp = Stamp.Match(line);
            if (!stamp.Success || long.Parse(stamp.Groups[1].Value) < since) return false;
            Match frame = FrameLine.Match(line);
            if (frame.Success)
            {
                Frames++;
                long atMs = long.Parse(frame.Groups[1].Value);
                LagsMs.Add(nowMs - atMs);
                bool steady = Steady == null || frame.Groups[4].Value == Steady;
                if (steady && !on) { on = true; onSinceMs = atMs; }
                else if (!steady && on) { on = false; onMs += atMs - onSinceMs; }
                lastFrameMs = atMs;
                AimX = (int)Math.Round((int.Parse(frame.Groups[2].Value) + halfWidth) * scale);
                AimY = (int)Math.Round((int.Parse(frame.Groups[3].Value) + halfHeight) * scale);
                return true;
            }
            if (line.Contains("region rebuild"))
            {
                Rebuilds.Add(line);
                if (on) Counted.Add(line);
            }
            return false;
        }

        public void Dispose()
        {
            if (stream != null) stream.Dispose();
            stream = null;
        }
    }

    // The manifest of a scenario's bench copy: the [cursor] table and every
    // Behavior go, one Behavior plays the steady animation 50 times over, and
    // that animation loses its loop key so a once-only one (react) cycles. A
    // scale above 0 replaces the manifest's. Mirrors the awk in
    // scripts/measure-mask-rebuild-x11.sh.
    public static class BenchManifest
    {
        static readonly Regex Table = new Regex(@"^\[");
        static readonly Regex Dropped = new Regex(@"^\[(cursor|behaviors\.[^\]]*)\]$");
        static readonly Regex Loop = new Regex(@"^loop\s*=");
        static readonly Regex Scale = new Regex(@"^scale\s*=");

        public static string Rewrite(string text, string steady, int scale)
        {
            List<string> output = new List<string>();
            if (scale > 0) output.Add("scale = " + scale);
            bool skip = false;
            string section = "";
            foreach (string raw in text.Split('\n'))
            {
                string line = raw.TrimEnd('\r');
                if (Table.IsMatch(line)) { skip = Dropped.IsMatch(line); section = line; }
                if (skip) continue;
                if (section == "[animations." + steady + "]" && Loop.IsMatch(line)) continue;
                if (scale > 0 && Scale.IsMatch(line)) continue;
                output.Add(line);
            }
            string[] play = new string[50];
            for (int i = 0; i < play.Length; i++) play[i] = "\"" + steady + "\"";
            output.Add("");
            output.Add("[behaviors.bench]");
            output.Add("play = [" + string.Join(", ", play) + "]");
            return string.Join("\n", output.ToArray());
        }
    }

    // fidget is per-monitor DPI aware and logs points: physical pixels over the
    // primary display's scale. These calls run per-monitor aware too, so
    // SetCursorPos takes physical pixels whatever PowerShell's own awareness is.
    public static class Screen
    {
        static readonly IntPtr PerMonitorAwareV2 = new IntPtr(-4);

        [StructLayout(LayoutKind.Sequential)]
        struct Point { public int X; public int Y; }

        [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
        [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
        [DllImport("user32.dll")] static extern IntPtr MonitorFromPoint(Point pt, uint flags);
        [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr monitor, int type, out uint dpiX, out uint dpiY);

        public static double PrimaryScale()
        {
            IntPtr previous = SetThreadDpiAwarenessContext(PerMonitorAwareV2);
            try
            {
                uint dpiX, dpiY;
                // 1: MONITOR_DEFAULTTOPRIMARY. 0: MDT_EFFECTIVE_DPI.
                if (GetDpiForMonitor(MonitorFromPoint(new Point(), 1), 0, out dpiX, out dpiY) != 0 || dpiX == 0) return 0;
                return dpiX / 96.0;
            }
            finally { if (previous != IntPtr.Zero) SetThreadDpiAwarenessContext(previous); }
        }

        public static bool MoveTo(int x, int y)
        {
            IntPtr previous = SetThreadDpiAwarenessContext(PerMonitorAwareV2);
            try { return SetCursorPos(x, y); }
            finally { if (previous != IntPtr.Zero) SetThreadDpiAwarenessContext(previous); }
        }
    }

    // Blocks until the desktop compositor presents its next frame, once per
    // display refresh: fidget's frames follow the same refresh, so one read
    // per wake keeps up with them. DwmFlush needs no adapter handle, so it
    // works whatever the display setup.
    public static class Refresh
    {
        [DllImport("dwmapi.dll")] static extern int DwmFlush();

        public static void Wait()
        {
            int status = DwmFlush();
            if (status != 0) throw new InvalidOperationException("DwmFlush failed: 0x" + status.ToString("x8"));
        }
    }
}
