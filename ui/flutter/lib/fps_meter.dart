import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';

/// One rolling sample of this app's frame pace, derived from Flutter's own
/// frame timing callbacks (the same source the platform Choreographer uses).
/// This measures the app's rendering, not a system-wide game FPS; that needs
/// privileges this APK deliberately does not request.
class DashFpsSample {
  const DashFpsSample({
    required this.fps,
    required this.min,
    required this.low1,
    required this.dropped,
    required this.valid,
  });

  final double fps;
  final double min;
  final double low1;
  final int dropped;
  final bool valid;

  static const DashFpsSample empty = DashFpsSample(
    fps: 0,
    min: 0,
    low1: 0,
    dropped: 0,
    valid: false,
  );
}

/// Pure statistics. Kept separate from the widget so it can be tested without
/// a live vsync. Frame intervals come from consecutive frame start stamps.
class DashFpsMeterModel {
  const DashFpsMeterModel._();

  static DashFpsSample compute(
    List<FrameTiming> frames, {
    int targetHz = 120,
  }) {
    if (frames.length < 2) return DashFpsSample.empty;

    final List<int> starts =
        frames.map((FrameTiming f) => f.timestampInMicroseconds).toList()
          ..sort();
    final int spanUs = starts.last - starts.first;
    if (spanUs <= 0) return DashFpsSample.empty;

    final List<double> intervals = <double>[];
    for (int i = 1; i < starts.length; i++) {
      intervals.add((starts[i] - starts[i - 1]) / 1000.0); // milliseconds
    }

    final double spanMs = spanUs / 1000.0;
    final double fps = (intervals.length) * 1000.0 / spanMs;
    final double min = 1000.0 / intervals.reduce((a, b) => a > b ? a : b);

    // 1% low: the average of the worst 1% of frame intervals.
    final List<double> worst = intervals.toList()
      ..sort((a, b) => b.compareTo(a));
    final int n = (worst.length * 0.01).clamp(1, worst.length);
    final double lowInterval =
        worst.sublist(0, n).reduce((a, b) => a + b) / n;
    final double low1 = 1000.0 / lowInterval;

    final double budget = 1000.0 / targetHz;
    final int dropped = frames
        .where(
          (FrameTiming f) =>
              f.totalSpan.inMicroseconds > budget * 1000.0 ||
              f.rasterDuration.inMicroseconds > budget * 1000.0,
        )
        .length;

    return DashFpsSample(
      fps: fps,
      min: min,
      low1: low1,
      dropped: dropped,
      valid: true,
    );
  }
}

/// Compact in-app overlay chip. Reads frame timings without producing extra
/// frames; a 500 ms timer refreshes the readout, so an idle app shows a stale
/// reading rather than spinning a perpetual ticker.
class DashFpsMeter extends StatefulWidget {
  const DashFpsMeter({required this.accent, super.key});

  final Color accent;

  @override
  State<DashFpsMeter> createState() => _DashFpsMeterState();
}

class _DashFpsMeterState extends State<DashFpsMeter> {
  final List<FrameTiming> _frames = <FrameTiming>[];
  Timer? _refresh;
  DashFpsSample _sample = DashFpsSample.empty;
  int _targetHz = 60;
  bool _attached = false;

  @override
  void initState() {
    super.initState();
    _targetHz = _refreshRate();
    _attached = true;
    SchedulerBinding.instance.addTimingsCallback(_collect);
    _refresh = Timer.periodic(const Duration(milliseconds: 500), (_) {
      if (!mounted) return;
      final DashFpsSample sample = DashFpsMeterModel.compute(
        _frames,
        targetHz: _targetHz,
      );
      setState(() => _sample = sample);
    });
  }

  int _refreshRate() {
    try {
      // platformDispatcher is stable across all supported Flutter versions.
      final double physical =
          WidgetsBinding.instance.platformDispatcher.views[0].physicalSize.width;
      final double logical =
          MediaQuery.viewLogicalSizeOf(WidgetsBinding.instance).width;
      if (logical > 0) {
        final double density = physical / logical;
        final int dpI = density.round();
        // Common mobile densities map to typical refresh rates.
        if (dpI <= 2) return 60;
        if (dpI <= 3) return 120;
      }
      return 60;
    } catch (_) {
      return 60;
    }
  }

  void _collect(List<FrameTiming> timings) {
    _frames.addAll(timings);
    // Keep only the last second of samples; older frames are stale.
    if (_frames.length > 240) {
      _frames.removeRange(0, _frames.length - 240);
    }
  }

  @override
  void dispose() {
    _refresh?.cancel();
    if (_attached) {
      SchedulerBinding.instance.removeTimingsCallback(_collect);
      _attached = false;
    }
    super.dispose();
  }

  String _text(double value) => value.toStringAsFixed(0);

  @override
  Widget build(BuildContext context) {
    final bool dark = Theme.of(context).brightness == Brightness.dark;
    final Color bg = dark
        ? const Color(0xE6000000)
        : const Color(0xE6FFFFFF);
    final Color fg = dark ? const Color(0xFFF4F7FB) : const Color(0xFF111925);
    final Color sub = dark
        ? const Color(0xFFAEB8C5)
        : const Color(0xFF566477);

    return RepaintBoundary(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(12, 8, 12, 0),
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(10),
            border: Border.all(color: widget.accent.withValues(alpha: 0.5)),
            boxShadow: <BoxShadow>[
              BoxShadow(
                color: Colors.black.withValues(alpha: dark ? 0.4 : 0.12),
                blurRadius: 8,
                offset: const Offset(0, 2),
              ),
            ],
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
            child: _sample.valid
                ? Row(
                    mainAxisSize: MainAxisSize.min,
                    children: <Widget>[
                      Text(
                        '${_text(_sample.fps)}',
                        style: TextStyle(
                          color: widget.accent,
                          fontSize: 18,
                          fontWeight: FontWeight.w800,
                          height: 1.0,
                        ),
                      ),
                      Text(
                        ' FPS',
                        style: TextStyle(
                          color: fg,
                          fontSize: 11,
                          fontWeight: FontWeight.w600,
                          height: 1.4,
                        ),
                      ),
                      const SizedBox(width: 8),
                      Text(
                        'avg ${_text(_sample.fps)}  min ${_text(_sample.min)}  '
                        '1% ${_text(_sample.low1)}  drop ${_sample.dropped}',
                        style: TextStyle(
                          color: sub,
                          fontSize: 10,
                          height: 1.3,
                        ),
                      ),
                    ],
                  )
                : Text(
                    'FPS \u2014',
                    style: TextStyle(
                      color: sub,
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
          ),
        ),
      ),
    );
  }
}
