---
name: dart
description: Use for Dart language patterns, best practices, and common utilities. Covers async patterns, streams, type system, testing, and performance optimization.
---

# Dart Language Patterns

## Async & Concurrency

### Future Patterns
```dart
// Sequential
final a = await fetchA();
final b = await fetchB(a);

// Parallel (independent)
final results = await Future.wait([
  fetchA(),
  fetchB(),
  fetchC(),
]);

// With timeout
final result = await fetchData().timeout(Duration(seconds: 5));

// Retry with exponential backoff
Future<T> retry<T>(Future<T> Function() fn, {int maxRetries = 3}) async {
  for (int i = 0; ; i++) {
    try {
      return await fn();
    } catch (e) {
      if (i >= maxRetries) rethrow;
      await Future.delayed(Duration(milliseconds: 100 * (1 << i)));
    }
  }
}
```

### Stream Patterns
```dart
// Transform
stream
  .where((event) => event.isValid)
  .map((event) => event.data)
  .distinct()
  .debounce(Duration(milliseconds: 300))
  .listen(handleData);

// Combine streams
final combined = Rx.combineLatest3(streamA, streamB, streamC, (a, b, c) => (a, b, c));

// Broadcast from single subscription
final broadcast = stream.asBroadcastStream();

// Cleanup
final subscription = stream.listen(handle);
subscription.cancel();
```

## Type System

### Generics & Constraints
```dart
// Generic with constraint
T max<T extends Comparable<T>>(T a, T b) => a.compareTo(b) > 0 ? a : b;

// Generic method with multiple bounds
void process<T extends Comparable<T> & Serializable>(T item) { }

// Reified generics (with dart:mirrors - limited in Flutter)
```

### Sealed Classes (Dart 3.0+)
```dart
sealed class Result<T> {
  const Result();
}

class Success<T> extends Result<T> {
  final T value;
  const Success(this.value);
}

class Error<T> extends Result<T> {
  final String message;
  const Error(this.message);
}

// Exhaustive switching
switch (result) {
  case Success(value: final v): print('Success: $v');
  case Error(message: final m): print('Error: $m');
}
```

### Records (Dart 3.0+)
```dart
// Anonymous records
(int, String, bool) user = (1, 'Alice', true);
final (id, name, active) = user;

// Named records
({int id, String name, bool active}) user = (id: 1, name: 'Alice', active: true);

// Return multiple values
({int status, String body}) fetch() => (status: 200, body: 'OK');
```

## Functional Patterns

### Collection Operations
```dart
final numbers = [1, 2, 3, 4, 5];

// Map/Filter/Reduce
final doubled = numbers.map((n) => n * 2).toList();
final evens = numbers.where((n) => n.isEven).toList();
final sum = numbers.reduce((a, b) => a + b);
final product = numbers.fold(1, (a, b) => a * b);

// Group by
final grouped = users.groupBy((u) => u.department);

// Chunk
final chunks = list.chunked(3);

// Distinct by key
final unique = users.map((u) => u.id).toSet().map((id) => users.firstWhere((u) => u.id == id));
```

### Extension Methods
```dart
extension IterableExtensions<T> on Iterable<T> {
  T? firstWhereOrNull(bool Function(T) test) {
    for (final element in this) {
      if (test(element)) return element;
    }
    return null;
  }
  
  Map<K, List<T>> groupBy<K>(K Function(T) key) {
    final map = <K, List<T>>{};
    for (final element in this) {
      map.putIfAbsent(key(element), () => []).add(element);
    }
    return map;
  }
}

// Usage
final evens = numbers.where((n) => n.isEven).toList();
final grouped = users.groupBy((u) => u.role);
```

## Error Handling

### Result Type Pattern
```dart
sealed class Result<T, E> {
  const Result();
  
  T getOrThrow() => switch (this) {
    Ok(value: final v) => v,
    Err(error: final e) => throw e,
  };
  
  T unwrapOr(T defaultValue) => switch (this) {
    Ok(value: final v) => v,
    Err(_) => defaultValue,
  };
  
  Result<U, E> map<U>(U Function(T) fn) => switch (this) {
    Ok(value: final v) => Ok(fn(v)),
    Err(error: final e) => Err(e),
  };
}

class Ok<T, E> extends Result<T, E> {
  final T value;
  const Ok(this.value);
}

class Err<T, E> extends Result<T, E> {
  final E error;
  const Err(this.error);
}
```

### Try/Catch with Pattern Matching
```dart
try {
  await riskyOperation();
} on FormatException catch (e) {
  logger.e('Format error: $e');
} on SocketException catch (e) {
  logger.e('Network error: $e');
} catch (e, stack) {
  logger.e('Unexpected: $e', stackTrace: stack);
} finally {
  cleanup();
}
```

## Performance

### Avoid Allocations in Hot Paths
```dart
// Bad: creates new list each iteration
for (final item in items) {
  final copy = [...items]; // Allocates!
  process(copy);
}

// Good: reuse buffer
final buffer = <int>[];
for (final item in items) {
  buffer.clear();
  buffer.addAll(items);
  process(buffer);
}
```

### Const Constructors
```dart
class Config {
  final int timeout;
  final String endpoint;
  
  const Config({required this.timeout, required this.endpoint});
}

// Usage - creates compile-time constant
static const defaultConfig = Config(timeout: 30, endpoint: 'api.example.com');
```

### Lazy Initialization
```dart
late final ExpensiveService _service = ExpensiveService();

// Or with initialization logic
late final Database _db = () {
  final db = Database();
  db.init();
  return db;
}();
```

## Testing Patterns

### Mockito
```dart
@GenerateMocks([UserRepository, ApiService])
void main() {
  late MockUserRepository mockRepo;
  late UserService service;
  
  setUp(() {
    mockRepo = MockUserRepository();
    service = UserService(mockRepo);
  });
  
  test('getUser returns user', () async {
    when(mockRepo.getUser('123')).thenAnswer((_) async => User(id: '123', name: 'Test'));
    
    final user = await service.getUser('123');
    
    expect(user.name, 'Test');
    verify(mockRepo.getUser('123')).called(1);
  });
}
```

### Golden File Testing
```dart
testWidgets('Button renders correctly', (tester) async {
  await tester.pumpWidget(MyButton(text: 'Click me'));
  await expectLater(
    find.byType(ElevatedButton),
    matchesGoldenFile('goldens/button.png'),
  );
}
```

## Performance Tips

### 1. Use `const` wherever possible
### 2. Avoid `setState` in build method
### 3. Use `ListView.builder` for large lists
### 4. Profile with `flutter run --profile` + DevTools
### 4. Use `RepaintBoundary` for static widgets
```dart
RepaintBoundary(
  child: ExpensiveStaticWidget(),
)
```

## Useful Packages
```yaml
dependencies:
  freezed_annotation: ^2.4.1  # Code generation for immutable classes
  json_annotation: ^4.8.1     # JSON serialization
  riverpod: ^2.4.9            # State management
  dio: ^5.4.0                 # HTTP client
  freezed: ^2.4.5             # Build runner
  build_runner: ^2.4.7        # Code generation

dev_dependencies:
  mockito: ^5.4.3
  build_runner: ^2.4.7
  flutter_test:
    sdk: flutter
```

## Build Runner Commands
```bash
# Generate code
dart run build_runner build --delete-conflicting-outputs

# Watch mode
dart run build_runner watch --delete-conflicting-outputs
```