---
name: flutter
description: Use for Flutter app development, patterns, testing, and platform integration. Covers Flutter architecture, state management, platform channels, and platform-specific integrations.
---

# Flutter Development Patterns

## Project Structure (Dash Essential Flutter App)
```
ui/flutter/
├── lib/
│   ├── main.dart                 # App entry point
│   ├── release_identity.dart     # Version codename
│   ├── models/                   # Data models
│   ├── screens/                  # UI screens
│   ├── widgets/                  # Reusable widgets
│   ├── services/                 # Platform channels, API
│   ├── providers/                # State management (Riverpod/Provider)
│   └── utils/                    # Helpers, constants
├── test/                         # Unit/widget tests
├── integration_test/             # Integration tests
├── android/                      # Android-specific config
├── ios/                          # iOS-specific config
└── pubspec.yaml                  # Dependencies
```

## State Management (Riverpod/Provider)
```dart
// Provider example
final gpuProfileProvider = StateNotifierProvider<GpuProfileNotifier, GpuProfileState>((ref) {
  return GpuProfileNotifier(ref.read(platformChannelProvider));
});

class GpuProfileNotifier extends StateNotifier<GpuProfileState> {
  GpuProfileNotifier(this._channel) : super(GpuProfileState.initial());
  
  final MethodChannel _channel;
  
  Future<void> setProfile(int profile) async {
    state = state.copyWith(isLoading: true);
    try {
      await _channel.invokeMethod('setPerfProfile', {'profile': profile});
      state = state.copyWith(currentProfile: profile, isLoading: false);
    } catch (e) {
      state = state.copyWith(error: e.toString(), isLoading: false);
    }
  }
}
```

## Platform Channel (Flutter ↔ Rust Daemon)
```dart
// Dart side
static const MethodChannel _channel = MethodChannel('dash_essential');

static Future<Map<String, dynamic>> getHealth() async {
  final result = await _channel.invokeMethod('GET health');
  return Map<String, dynamic>.from(result);
}

static Future<void> setPerfProfile(int profile) async {
  await _channel.invokeMethod('SET perf $profile');
}

static Future<void> setTouchRate(int rate) async {
  await _channel.invokeMethod('SET touch $rate');
}
```

```rust
// Rust side (lib.rs)
fn handle_command(line: &str) -> String {
    match line {
        "GET health" => Ok(diagnostics_health_json()),
        cmd if cmd.starts_with("SET perf ") => {
            let profile = cmd.split_whitespace().nth(2).unwrap().parse().unwrap();
            transition_service(&format!("SET perf {}", profile))
        }
        cmd if cmd.starts_with("SET touch ") => {
            let rate = cmd.split_whitespace().nth(2).unwrap().parse().unwrap();
            apply_dash_touch_rate(rate).map(|_| "OK applied".into())
        }
        _ => Err("unknown command".into()),
    }
}
```

## Per-App Profile UI
```dart
class PerAppProfileScreen extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profiles = ref.watch(perAppProfilesProvider);
    final foreground = ref.watch(foregroundAppProvider);
    
    return ListView(
      children: profiles.map((p) => ProfileTile(
        package: p.package,
        profile: p.profile,
        isForeground: p.package == foreground,
        onTap: () => ref.read(perAppProfilesProvider.notifier).update(p.package, newProfile),
      )).toList(),
    );
  }
}
```

## Testing

### Unit Tests
```dart
// test/gpu_profile_notifier_test.dart
void main() {
  group('GpuProfileNotifier', () {
    late MockMethodChannel mockChannel;
    late GpuProfileNotifier notifier;
    
    setUp(() {
      mockChannel = MockMethodChannel();
      notifier = GpuProfileNotifier(mockChannel);
    });
    
    test('setProfile calls platform channel', () async {
      when(mockChannel.invokeMethod('SET perf 1')).thenAnswer((_) async => 'OK');
      
      await notifier.setProfile(1);
      
      verify(mockChannel.invokeMethod('SET perf 1')).called(1);
      expect(notifier.state.currentProfile, 1);
    });
  });
}
```

### Widget Tests
```dart
// test/screens/per_app_profile_screen_test.dart
void main() {
  testWidgets('PerAppProfileScreen shows borrowed profile', (tester) async {
    await tester.pumpWidget(ProviderScope(overrides: [
      perAppProfilesProvider.overrideWith((ref) => AsyncValue.data([...])),
      foregroundAppProvider.overrideWith((ref) => 'com.test.app'),
    ], child: MaterialApp(home: PerAppProfileScreen())));
    
    expect(find.text('com.test.app'), findsOneWidget);
    expect(find.text('Borrowed'), findsOneWidget);
  });
}
```

### Integration Tests
```dart
// integration_test/app_test.dart
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  
  testWidgets('Full flow: set per-app profile', (tester) async {
    app.main();
    await tester.pumpAndSettle();
    
    // Navigate to per-app
    await tester.tap(find.byIcon(Icons.apps));
    await tester.pumpAndSettle();
    
    // Select app
    await tester.tap(find.text('Minecraft'));
    await tester.pumpAndSettle();
    
    // Set profile
    await tester.tap(find.text('Performance'));
    await tester.pumpAndSettle();
    
    // Verify health reflects change
    final health = await MethodChannel('dash_essential').invokeMethod('GET health');
    expect(health['perApp']['ownedControls'], contains('Gpu'));
  });
}
```

## Build Commands
```bash
# Debug
flutter build apk --debug

# Release (requires keystore)
flutter build apk --release

# Bundle for Play Store
flutter build appbundle --release

# Web
flutter build web --release

# Run tests
flutter test
flutter test integration_test/
flutter test --coverage
genhtml coverage/lcov.info -o coverage/html
```

## Platform-Specific Code

### Android (MethodChannel)
```kotlin
// MainActivity.kt
class MainActivity : FlutterActivity() {
    private val CHANNEL = "dash_essential"
    
    override fun configureFlutterEngine(engine: FlutterEngine) {
        super.configureFlutterEngine(engine)
        MethodChannel(engine.dartExecutor.binaryMessenger, CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "GET health" -> result.success(getHealthJson())
                "SET perf" -> { setPerfProfile(call.argument<Int>("profile")); result.success("OK") }
                "SET touch" -> { setTouchRate(call.argument<Int>("rate")); result.success("OK") }
                else -> result.notImplemented()
            }
        }
    }
}
```

### Android Permissions
```xml
<!-- AndroidManifest.xml -->
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
<!-- For TCP IPC with daemon -->
<uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
```

## Common Patterns

### Async State with Riverpod
```dart
final healthProvider = FutureProvider.autoDispose<Health>((ref) async {
  final channel = ref.read(platformChannelProvider);
  final result = await channel.invokeMethod('GET health');
  return Health.fromJson(result);
});

// In widget
final healthAsync = ref.watch(healthProvider);
return healthAsync.when(
  data: (health) => HealthView(health: health),
  loading: () => CircularProgressIndicator(),
  error: (e, _) => Text('Error: $e'),
);
```

### Platform Channel Error Handling
```dart
Future<T> safeInvoke<T>(String method, [dynamic arguments]) async {
  try {
    return await _channel.invokeMethod<T>(method, arguments);
  } on PlatformException catch (e) {
    throw DaemonException(e.code, e.message ?? 'Unknown error');
  }
}
```

### Release Identity
```dart
// lib/release_identity.dart
const String dashVersionName = '1.18.16';
const int dashVersionCode = 11816;
const String dashReleaseCodename = 'Tsuki';
```

## Debugging
```bash
# Flutter doctor
flutter doctor -v

# Device logs
adb logcat -s flutter

# Dart DevTools
flutter pub global run devtools

# Profile mode
flutter run --profile
```