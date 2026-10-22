---
name: flutter-patterns
description: Use for Flutter-specific architectural patterns, widget composition, navigation, theming, and platform integration. Covers common Flutter app architectures and best practices.
---

# Flutter Architectural Patterns

## App Architecture (Clean Architecture)

```
lib/
├── core/
│   ├── constants/          # App-wide constants
│   ├── errors/             # Failure classes, exceptions
│   ├── utils/              # Helpers, extensions
│   └── themes/             # ThemeData, colors, text styles
├── data/
│   ├── datasources/        # Remote (API), Local (DB, prefs)
│   ├── models/             # DTOs, entity mappings
│   └── repositories/       # Repository implementations
├── domain/
│   ├── entities/           # Business objects
│   ├── repositories/       # Abstract repository interfaces
│   └── usecases/           # Business logic (single responsibility)
├── presentation/
│   ├── providers/          # State management (Riverpod/Bloc)
│   ├── screens/            # Full-screen pages
│   ├── widgets/            # Reusable UI components
│   └── navigation/         # Router, route guards
└── main.dart               # App entry, DI setup
```

## Dependency Injection (Riverpod)

```dart
// providers.dart
final platformChannelProvider = Provider<MethodChannel>((ref) {
  return const MethodChannel('dash_essential');
});

final healthRepositoryProvider = Provider<HealthRepository>((ref) {
  return HealthRepositoryImpl(ref.read(platformChannelProvider));
});

final healthProvider = FutureProvider.autoDispose<Health>((ref) async {
  return ref.read(healthRepositoryProvider).getHealth();
});

// Per-app profiles
final perAppProfilesProvider = StateNotifierProvider<PerAppProfilesNotifier, AsyncValue<List<AppProfile>>>((ref) {
  return PerAppProfilesNotifier(ref.read(perAppProfilesRepositoryProvider));
});

final foregroundAppProvider = StreamProvider<String?>((ref) {
  return ref.read(foregroundAppServiceProvider).watchForegroundApp();
});
```

## Repository Pattern

```dart
// Domain
abstract class HealthRepository {
  Future<Health> getHealth();
  Future<void> setPerfProfile(int profile);
  Future<void> setTouchRate(int rate);
}

// Data
class HealthRepositoryImpl implements HealthRepository {
  final MethodChannel _channel;
  
  HealthRepositoryImpl(this._channel);
  
  @override
  Future<Health> getHealth() async {
    final result = await _channel.invokeMethod('GET health');
    return Health.fromJson(Map<String, dynamic>.from(result));
  }
  
  @override
  Future<void> setPerfProfile(int profile) async {
    await _channel.invokeMethod('SET perf $profile');
  }
}
```

## Navigation (go_router)

```dart
final router = GoRouter(
  initialLocation: '/dashboard',
  routes: [
    GoRoute(
      path: '/dashboard',
      builder: (context, state) => DashboardScreen(),
    ),
    GoRoute(
      path: '/per-app',
      builder: (context, state) => PerAppProfileScreen(),
    ),
    GoRoute(
      path: '/settings',
      builder: (context, state) => SettingsScreen(),
    ),
    GoRoute(
      path: '/profile/:packageName',
      builder: (context, state) => AppProfileDetailScreen(
        packageName: state.pathParameters['packageName']!,
      ),
    ),
  ],
  errorBuilder: (context, state) => Scaffold(
    body: Center(child: Text('Page not found: ${state.error}')),
  ),
);
```

## Theme System

```dart
class AppTheme {
  static ThemeData light = ThemeData(
    useMaterial3: true,
    colorScheme: ColorScheme.fromSeed(
      seedColor: Color(0xFF6750A4),
      brightness: Brightness.light,
    ),
    textTheme: TextTheme(
      headlineLarge: TextStyle(fontSize: 32, fontWeight: FontWeight.w700),
      bodyLarge: TextStyle(fontSize: 16, height: 1.5),
    ),
    elevatedButtonTheme: ElevatedButtonThemeData(
      style: ElevatedButton.styleFrom(
        padding: EdgeInsets.symmetric(horizontal: 24, vertical: 12),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
      ),
    ),
  );
  
  static ThemeData dark = light.copyWith(
    colorScheme: ColorScheme.fromSeed(
      seedColor: Color(0xFF6750A4),
      brightness: Brightness.dark,
    ),
  );
}
```

## Widget Composition Patterns

### Compound Components
```dart
class ProfileCard extends StatelessWidget {
  final String packageName;
  final AppProfile profile;
  final bool isForeground;
  final VoidCallback onTap;
  final VoidCallback? onEdit;
  
  const ProfileCard({
    required this.packageName,
    required this.profile,
    required this.isForeground,
    required this.onTap,
    this.onEdit,
  });
  
  @override
  Widget build(BuildContext context) {
    return Card(
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(child: Text(packageName, style: Theme.of(context).textTheme.titleMedium)),
                  if (isForeground) Chip(label: Text('Active', style: TextStyle(fontSize: 12))),
                ],
              ),
              SizedBox(height: 8),
              _buildProfileRow('GPU', profile.gpu),
              _buildProfileRow('Refresh', profile.refresh),
            ],
          ),
        ),
      ),
    );
  }
  
  Widget _buildProfileRow(String label, int value) => Padding(
    padding: EdgeInsets.symmetric(vertical: 4),
    child: Row(
      children: [
        Text('$label: ', style: TextStyle(color: Colors.grey[600])),
        Text('$value', style: TextStyle(fontWeight: FontWeight.w600)),
      ],
    ),
  );
}
```

### Sliver-Based Scrolling
```dart
CustomScrollView(
  slivers: [
    SliverAppBar(
      title: Text('Dash Essential'),
      floating: true,
      pinned: true,
    ),
    SliverList(
      delegate: SliverChildBuilderDelegate(
        (context, index) => ProfileCard(...),
        childCount: profiles.length,
      ),
    ),
  ],
)
```

## Platform Channel Integration

### MethodChannel Wrapper
```dart
class DashEssentialChannel {
  static const MethodChannel _channel = MethodChannel('dash_essential');
  
  static Future<Health> getHealth() async {
    final result = await _channel.invokeMethod('GET health');
    return Health.fromJson(Map<String, dynamic>.from(result));
  }
  
  static Future<void> setPerfProfile(int profile) async {
    await _channel.invokeMethod('SET perf $profile');
  }
  
  static Future<void> setTouchRate(int rate) async {
    await _channel.invokeMethod('SET touch $rate');
  }
  
  static Future<void> setDisplayColor(int mode) async {
    await _channel.invokeMethod('SET display.color $mode');
  }
  
  static Future<void> setDisplayTemp(int mode) async {
    await _channel.invokeMethod('SET display.temp $mode');
  }
}
```

### Error Handling
```dart
class DaemonException implements Exception {
  final String code;
  final String message;
  
  DaemonException(this.code, this.message);
  
  @override
  String toString() => 'DaemonException: $code - $message';
}

Future<T> safeInvoke<T>(String method, [dynamic arguments]) async {
  try {
    return await _channel.invokeMethod<T>(method, arguments);
  } on PlatformException catch (e) {
    throw DaemonException(e.code, e.message ?? 'Unknown error');
  }
}
```

## State Management (Riverpod)

### Notifier for Per-App Profiles
```dart
class PerAppProfilesNotifier extends StateNotifier<AsyncValue<List<AppProfile>>> {
  final PerAppProfilesRepository _repository;
  
  PerAppProfilesNotifier(this._repository) : super(const AsyncValue.loading()) {
    load();
  }
  
  Future<void> load() async {
    state = const AsyncValue.loading();
    try {
      final profiles = await _repository.getAll();
      state = AsyncValue.data(profiles);
    } catch (e, st) {
      state = AsyncValue.error(e, st);
    }
  }
  
  Future<void> update(String package, AppProfile profile) async {
    state = AsyncValue.data(state.value!.map((p) => p.package == package ? profile : p).toList());
    await _repository.save(package, profile);
    // Notify daemon
    await DaemonChannel.setAppProfile(package, profile);
  }
}
```

### AsyncValue UI Pattern
```dart
class ProfileList extends ConsumerWidget {
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profilesAsync = ref.watch(perAppProfilesProvider);
    
    return profilesAsync.when(
      data: (profiles) => ListView.builder(
        itemCount: profiles.length,
        itemBuilder: (context, index) => ProfileCard(profile: profiles[index]),
      ),
      loading: () => Center(child: CircularProgressIndicator()),
      error: (e, st) => Center(child: Text('Error: $e')),
    );
  }
}
```

## Testing Widgets

### Widget Test with Provider Overrides
```dart
void main() {
  testWidgets('Dashboard shows health data', (tester) async {
    final mockHealth = Health(
      gpu: Gpu(profile: 2, liveMHz: 598),
      display: Display(route: 'vendor'),
    );
    
    await tester.pumpWidget(
      ProviderScope(overrides: [
        healthProvider.overrideWithValue(AsyncValue.data(mockHealth)),
      ], child: MaterialApp(home: DashboardScreen())),
    );
    
    await tester.pump();
    expect(find.text('598 MHz'), findsOneWidget);
    expect(find.text('Vendor'), findsOneWidget);
  });
}
```

### Golden File Testing
```dart
testWidgets('ProfileCard golden test', (tester) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: AppTheme.light,
      home: ProfileCard(
        packageName: 'com.test.app',
        profile: AppProfile(gpu: 1, refresh: 120),
        isForeground: true,
        onTap: () {},
      ),
    ),
  );
  
  await expectLater(
    find.byType(ProfileCard),
    matchesGoldenFile('goldens/profile_card.png'),
  );
}
```

## Performance Optimization

### 1. Const Constructors
```dart
const ProfileCard({...}); // All fields final
```

### 2. RepaintBoundary
```dart
RepaintBoundary(
  child: ExpensiveChartWidget(data: data),
)
```

### 3. ListView.builder for Large Lists
```dart
ListView.builder(
  itemCount: 1000,
  itemBuilder: (context, index) => ProfileCard(...),
)
```

### 4. Selective Rebuilds with Consumer
```dart
Consumer(builder: (context, ref, _) {
  final profile = ref.watch(selectedProfileProvider);
  return Text('Profile: $profile');
})
```

## Platform-Specific Code

### Android (Kotlin)
```kotlin
// MainActivity.kt
class MainActivity : FlutterActivity() {
    private val CHANNEL = "dash_essential"
    
    override fun configureFlutterEngine(engine: FlutterEngine) {
        MethodChannel(engine.dartExecutor.binaryMessenger, CHANNEL).setMethodCallHandler { call, result ->
            when (call.method) {
                "GET health" -> result.success(getHealthJson())
                "SET perf" -> { setPerfProfile(call.argument<Int>("profile")); result.success("OK") }
                else -> result.notImplemented()
            }
        }
    }
}
```

### iOS (Swift)
```swift
// AppDelegate.swift
let controller = window?.rootViewController as! FlutterViewController
let channel = FlutterMethodChannel(name: "dash_essential", binaryMessenger: controller.binaryMessenger)

channel.setMethodCallHandler { call, result in
    switch call.method {
    case "GET health":
        result(getHealthJson())
    case "SET perf":
        setPerfProfile(call.arguments as! Int)
        result("OK")
    default:
        result(FlutterMethodNotImplemented)
    }
}
```

## Build Flavors
```yaml
# pubspec.yaml
flutter:
  flavors:
    dev:
      app_name: "Dash Essential Dev"
    prod:
      app_name: "Dash Essential"
```

```bash
flutter run --flavor dev
flutter build apk --flavor prod --release
```

## Common Gotchas

1. **BuildContext across async gaps** - Capture before await
2. **ListView without keys** - Add keys for stable identity
3. **setState in build** - Move to initState or use callbacks
4. **Missing dispose** - Cancel subscriptions, controllers
5. **Large images** - Use `resizeImage` or `cached_network_image`