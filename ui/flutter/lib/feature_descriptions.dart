part of 'main.dart';

class FeatureDescription {
  const FeatureDescription(this.title, this.short, this.long);
  final String title;
  final String short;
  final String long;
}

const Map<String, FeatureDescription> kFeatureDescriptions = <String, FeatureDescription>{
  'mtk.frequency': FeatureDescription(
    'Fixed GPU frequency',
    'Locks the GPU to one frequency from its own OPP table until you release it.',
    'Picks the row matching your frequency from the GPU OPP table and writes it to the driver\'s fix node, so the GPU stops switching between steps. The pin outranks the maximum-frequency limit and is remembered and restored when you release it. The voltage-locked variant is refused by this phone\'s kernel, so use the plain pin. A wrong voltage can freeze the GPU; only a reboot recovers it.',
  ),
  'mtk.thermal_gpt': FeatureDescription(
    'Thermal governor',
    'Turns the kernel\'s GPU thermal limiter (`/sys/kernel/thermal/gpt`) off or on.',
    'The kernel normally throttles the GPU when it heats up. Disabling the governor removes that protection, letting the GPU hold its clocks longer under load at the cost of heat and battery. The original setting is journaled and restored with OEM Reset. Use together with a fixed GPU pin for benchmarking or sustained gaming, and re-enable it afterwards.',
  ),
  'mtk.fpsgo': FeatureDescription(
    'FPSGO',
    'MediaTek\'s frame booster: speeds the GPU and CPU up per frame to hold your target FPS.',
    'FPSGO (Frame Pacing Governor) watches frame times and raises clocks when frames run long, so games keep their target frame rate without locking frequencies. Leave it on for normal gaming; turning it off stops the driver\'s automatic frame boosts and gives you quieter, cooler behavior only if you control clocks manually.',
  ),
  'mtk.fbt_ceiling': FeatureDescription(
    'FBT ceiling',
    'Frame Boost Technology\'s cap on how far FPSGO may raise the GPU per frame.',
    'FBT sets a ceiling table: how high the GPU may climb for a heavy frame. With the ceiling off, FPSGO can push the GPU to its highest steps, improving peak frame rate at the cost of heat and battery. With it on, boosts stay moderate. This complements FPSGO — change them together if you tune frame pacing.',
  ),
  'mtk.perfmgr': FeatureDescription(
    'Perfmgr',
    'MediaTek\'s performance manager: applies per-app boost hints for the foreground app.',
    'Perfmgr requests CPU/GPU headroom when a demanding app takes the foreground, similar to a game-mode daemon. Turning it off removes those automatic hints; clocks then follow whatever else you set. Restorable with OEM Reset.',
  ),
  'mtk.ged_kpi': FeatureDescription(
    'GED KPI',
    'The GPU driver\'s frame-quality metric (KPI) feed used for boost decisions.',
    'GED is MediaTek\'s GPU extension driver; its KPI module tracks per-frame timing so the boost stack can react to real frame times. Leave it on unless you are testing whether a boost source misbehaves — most other GPU boosts depend on this data.',
  ),
  'mtk.dcs': FeatureDescription(
    'DCS',
    'GPU driver display-scaling mode: trades frame count for frequency.',
    'DCS modes tell the GPU how aggressively to drop to lower frequency/frame-count combinations when load allows. The menu lists the modes this kernel offers; pick one or Disable. The selected code is journaled and restored with OEM Reset.',
  ),
  'mtk.ddr': FeatureDescription(
    'Fixed DDR OPP',
    'Forces the memory (DRAM) to one speed step and holds it there.',
    'Writes the DVFS source controller\'s force node so DRAM stops scaling and stays at the chosen operating point — useful for memory benchmarks or stability tests. The node is write-only on some kernels, so the current value cannot be shown. Releasing restores the journaled value or the driver\'s off sentinel. A wrong step can freeze the device; only a reboot recovers it.',
  ),
  'clocks.memory': FeatureDescription(
    'LPDDR5X RAM',
    'Sets the DRAM frequency range, or locks an exact speed with a trial.',
    'Clamp the range the memory governor may use, or run an exact lock: the daemon applies it, watches stability for the trial period, and restores the previous state if you reject it. Everything is journaled — OEM Reset puts the driver\'s own values back.',
  ),
  'clocks.storage': FeatureDescription(
    'UFS 4.0 Storage',
    'Sets the storage controller\'s clock range the same way as RAM.',
    'Same range-and-lock mechanism as the memory section, applied to the UFS storage clock. Useful when testing I/O performance ceilings. Journaled and restorable with OEM Reset.',
  ),
  'perapp.preferences': FeatureDescription(
    'Profile preferences',
    'Switches that decide how this app\'s profile behaves.',
    'Per-app toggles such as boosting while the app is in front. The profile only applies while its app is the foreground app, and the daemon reverts everything the moment you leave the app.',
  ),
  'perapp.touch_display': FeatureDescription(
    'Touch & display',
    'Touch sampling rate and display settings for this app.',
    'Raises the touch panel\'s report rate and adjusts display behavior while the app runs, for lower input latency in games. Original values are restored when the app closes.',
  ),
  'perapp.mali_gpu': FeatureDescription(
    'Mali GPU',
    'GPU frequency bounds, governor and boost for this app.',
    'Sets the Mali GPU\'s minimum/maximum frequency and governor while this app is in front — for example a fixed floor for consistent frame pacing. Restored automatically on app exit.',
  ),
  'perapp.cpu_cores': FeatureDescription(
    'CPU cores',
    'Which CPU cores stay online and at what floor for this app.',
    'Keeps selected cores from sleeping and raises their minimum frequency while the app runs, so it always has warm cores available. Restored automatically when you leave the app.',
  ),
  'perapp.scheduler_clamps': FeatureDescription(
    'Scheduler clamps',
    'Scheduler uclamp minimums for CPU capacity while this app runs.',
    'uclamp raises the scheduler\'s perceived capacity of the app\'s tasks so they land on faster cores sooner. A softer alternative to pinning frequencies. Restored on app exit.',
  ),
  'perapp.frame_scheduler': FeatureDescription(
    'Frame scheduler',
    'FPS metering and frame-time targets for this app.',
    'Watches this app\'s frame times and can steer the boost stack toward its frame-rate target. Combine with FPSGO for game frame pacing. Restored on app exit.',
  ),
  'dexopt.scheduler': FeatureDescription(
    'DexOpt Scheduler',
    'Runs Android\'s background AOT app compilation when you choose, not when the OS feels like it.',
    'DEX optimization compiles apps to native code; stock devices do it at random times and steal performance from what you are doing. This scheduler runs the same work under your control (time window, cadence, per-package opt-out) so compilation happens while the phone idles instead of while you use it.',
  ),
  'conditionals.stack': FeatureDescription(
    'Conditional profiles',
    'Rules that switch profiles automatically, like "when charging" or "when this app is in front".',
    'Conditionals sit on top of per-app profiles: when a rule matches, its profile takes over until the rule stops matching. The daemon keeps exactly one active profile and reverts the previous one on every switch.',
  ),
  'diagnostics.logs': FeatureDescription(
    'Health & Logs',
    'Captures daemon and system logs into one bundle for bug reports.',
    'Collects the daemon\'s own log, kernel messages and selected system dumps into exportable files. Export the bundle before filing a bug — it shows which node failed and why.',
  ),
};

void showFeatureDescriptionSheet(BuildContext context, String id) {
  final FeatureDescription? entry = kFeatureDescriptions[id];
  if (entry == null) return;
  showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (BuildContext context) {
      final ColorScheme colors = Theme.of(context).colorScheme;
      return SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(22, 8, 22, 22),
          // Scrollable: the longest copy overflows a half-screen sheet on
          // small devices.
          child: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(
                  entry.title,
                  style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w800),
                ),
                const SizedBox(height: 10),
                Text(
                  entry.short,
                  style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
                ),
                const SizedBox(height: 14),
                Text(entry.long, style: const TextStyle(fontSize: 14, height: 1.45)),
              ],
            ),
          ),
        ),
      );
    },
  );
}

/// Title row with an info sheet, plus the always-visible short line.
/// Unknown ids fall back to the plain title so screens never crash on a
/// registry gap.
Widget featureCardHeader(BuildContext context, String id, {String? title}) {
  final FeatureDescription? entry = kFeatureDescriptions[id];
  if (entry == null) return Text(title ?? id, style: const TextStyle(fontWeight: FontWeight.w800));
  return Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: <Widget>[
      Row(
        children: <Widget>[
          Expanded(
            child: Text(
              title ?? entry.title,
              style: const TextStyle(fontWeight: FontWeight.w800),
            ),
          ),
          IconButton(
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.info_outline_rounded, size: 19),
            color: Theme.of(context).colorScheme.onSurfaceVariant,
            tooltip: 'About ${entry.title}',
            onPressed: () => showFeatureDescriptionSheet(context, id),
          ),
        ],
      ),
      const SizedBox(height: 4),
      Text(
        entry.short,
        style: TextStyle(fontSize: 11.5, color: Theme.of(context).colorScheme.onSurfaceVariant),
      ),
    ],
  );
}
