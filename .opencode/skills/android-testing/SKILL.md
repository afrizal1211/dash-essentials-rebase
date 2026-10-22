---
name: android-testing
description: Use for Android app testing strategies: unit tests (JUnit 5 + MockK), instrumented tests (Compose/Espresso), UI tests, integration testing for Tauri/Flutter apps, Gradle MCP workflows.
---

# Android Testing Skill

## Test Types & Frameworks

### Unit Tests (`src/test/`)
- **Framework**: JUnit 5 + MockK for Kotlin mocking
- **Coverage**: ViewModels, Use Cases, Repositories, Mappers
- **Pattern**: Given-When-Then with descriptive test names
```kotlin
@Test
fun `given valid input, when login called, then returns success`() {
    // Given
    val mockRepo = mockk()
    every { mockRepo.login(any()) } returns Result.success(User("test"))
    // When
    val result = LoginUseCase(mockRepo)("user", "pass")
    // Then
    assertTrue(result.isSuccess)
}
```

### Instrumented Tests (`src/androidTest/`)
- **Framework**: Compose UI Test + Espresso for legacy views
- **Scope**: UI interaction, navigation, database, permissions
- **Run via**: `./gradlew connectedCheck` or Gradle MCP `:app:connectedCheck`

### UI Tests (Compose)
```kotlin
@RunWith(AndroidJUnit4::class)
class LoginScreenTest {
    @get:Rule val composeTestRule = createComposeRule()
    
    @Test
    fun `displays error on invalid login`() {
        composeTestRule.setContent { LoginScreen(viewModel = mockViewModel()) }
        composeTestRule.onNodeWithText("Login").performClick()
        composeTestRule.onNodeWithText("Invalid credentials").assertIsDisplayed()
    }
}
```

### Flutter-Specific Testing
- Test Kotlin bridge methods with unit tests
- Verify `Invoke` handler mappings match Rust commands
- Test plugin callbacks with mock Rust bridge
- Integration test: `flutter test integration_test/`

## MCP Workflows (via Gradle MCP)

### Unit Tests
```bash
gradle run_task --task=:app:testDebugUnitTest
```

### Instrumented Tests
```bash
gradle run_task --task=:app:connectedDebugAndroidTest
```

### Coverage
```bash
gradle run_task --task=:app:testCoverage
# Report at app/build/reports/coverage/debug/index.html
```

### Via Mobile MCP (Device Automation)
```bash
# Launch app for manual verification
mobile install_app --package=io.github.neeschal.dashessential
mobile launch_app --package=io.github.neeschal.dashessential

# Screenshot verification
mobile screenshot --output=step1.png

# UI hierarchy inspection
mobile capture_ui_hierarchy --output=hierarchy.xml
```

## Tauri/Flutter-Specific Testing
- Test Kotlin bridge methods with unit tests
- Verify `Invoke` handler mappings match Rust commands
- Test plugin callbacks with mock Rust bridge
- Integration test: `flutter test integration_test/`

## Gradle Commands for This Project

### Run All Tests
```bash
cd android && ./gradlew test connectedCheck
```

### Run Specific Test Class
```bash
./gradlew :app:testDebugUnitTest --tests "com.example.DashEssentialTest"
```

### Run with Coverage
```bash
./gradlew jacocoTestReport
# Report: app/build/reports/jacoco/testDebugUnitTest/html/index.html
```

### Run Instrumented Tests on Device
```bash
# Requires device connected via ADB
./gradlew connectedDebugAndroidTest
```

## MockK Patterns for ViewModel Testing
```kotlin
@ExtendWith(MockKExtension::class)
class DashboardViewModelTest {
    @MockK lateinit var repo: DashboardRepository
    @MockK lateinit var dispatcher: TestCoroutineDispatcher
    
    @BeforeEach
    fun setup() {
        Dispatchers.setMain(dispatcher)
    }
    
    @AfterEach
    fun teardown() {
        Dispatchers.resetMain()
    }
    
    @Test
    fun `loadData emits loading then success`() = runTest {
        // Given
        coEvery { repo.getData() } returns flowOf(Result.success(Data("test")))
        
        // When
        val viewModel = DashboardViewModel(repo)
        val states = viewModel.uiState.toList()
        
        // Then
        assertEquals(listOf(Loading, Success("test")), states)
    }
}
```

## Espresso Patterns for Legacy Views
```kotlin
@Test
fun `click button navigates to detail`() {
    onView(withId(R.id.button)).perform(click())
    onView(withId(R.id.detail_text)).check(matches(isDisplayed()))
}
```

## Compose UI Testing Patterns
```kotlin
@RunWith(AndroidJUnit4::class)
class PerAppProfileScreenTest {
    @get:Rule val composeTestRule = createComposeRule()
    
    @Test
    fun `profile list shows borrowed apps`() {
        composeTestRule.setContent {
            PerAppProfileScreen(viewModel = mockViewModel())
        }
        composeTestRule
            .onNodeWithText("com.mojang.minecraftpe")
            .assertIsDisplayed()
    }
}
```

## CI Integration (GitHub Actions)
```yaml
- name: Run Android Tests
  run: |
    ./gradlew test connectedCheck --no-daemon
  env:
    GRADLE_OPTS: "-Dorg.gradle.jvmargs=-Xmx4g"
```

## Test Reports
- Unit: `app/build/reports/tests/testDebugUnitTest/index.html`
- Instrumented: `app/build/reports/androidTests/connected/index.html`
- Coverage: `app/build/reports/coverage/debug/index.html`

## Useful Gradle Flags
```bash
# Skip pre-dexing for faster builds
./gradlew test -Pandroid.dexOptions.preDexLibraries=false

# Max workers
./gradlew test --max-workers=4

# Debug test
./gradlew test --debug-jvm
```