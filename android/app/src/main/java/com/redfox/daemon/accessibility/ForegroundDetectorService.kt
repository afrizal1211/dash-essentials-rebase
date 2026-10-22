package com.redfox.daemon.accessibility

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.accessibility.AccessibilityEvent
import android.content.Intent
import android.content.Context
import android.os.Build
import android.util.Log
import java.lang.ref.WeakReference

/**
 * ForegroundDetectorService - AccessibilityService for detecting foreground app changes.
 *
 * Uses TYPE_WINDOW_STATE_CHANGED events to detect when the foreground app changes.
 * This is the same approach used by RedFox Prism for per-app profile switching.
 *
 * Events are broadcast via LocalBroadcastManager and also sent to the Rust daemon
 * via JNI for integration with the per-app profile system.
 */
class ForegroundDetectorService : AccessibilityService() {

    companion object {
        private const val TAG = "ForegroundDetector"
        private const val ACTION_FOREGROUND_APP_CHANGED = "com.redfox.daemon.FOREGROUND_APP_CHANGED"
        const val EXTRA_PACKAGE_NAME = "package_name"
        const val EXTRA_CLASS_NAME = "class_name"
        const val EXTRA_EVENT_TYPE = "event_type"

        // JNI bridge
        @Volatile
        private var sJniCallback: JniCallback? = null

        fun setJniCallback(callback: JniCallback) {
            sJniCallback = callback
        }

        interface JniCallback {
            fun onForegroundAppChanged(packageName: String, className: String)
        }
    }

    private var currentForegroundPackage: String? = null
    private var currentForegroundClass: String? = null

    override fun onServiceConnected() {
        super.onServiceConnected()
        Log.d(TAG, "ForegroundDetectorService connected")

        val info = AccessibilityServiceInfo().apply {
            // Only listen for window state changes (foreground app changes)
            eventTypes = AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED

            // Generic feedback (no audio/haptic feedback)
            feedbackType = AccessibilityServiceInfo.FEEDBACK_GENERIC

            // 100ms timeout to avoid event flooding
            notificationTimeout = 100

            // Monitor all packages
            packageNames = null

            // Include non-important views (system UI, etc.)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.JELLY_BEAN) {
                flags = AccessibilityServiceInfo.FLAG_INCLUDE_NOT_IMPORTANT_VIEWS
            }

            // Don't retrieve window content (privacy)
            canRetrieveWindowContent = false
        }
        serviceInfo = info

        Log.d(TAG, "ForegroundDetectorService configured: eventTypes=${info.eventTypes}, flags=${info.flags}")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent) {
        if (event.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) {
            handleWindowStateChanged(event)
        }
    }

    private fun handleWindowStateChanged(event: AccessibilityEvent) {
        val packageName = event.packageName?.toString()
        val className = event.className?.toString()

        // Filter out null/empty and our own package
        if (packageName.isNullOrBlank()) return
        if (packageName == packageName) return // our own package

        // Skip system UI and launcher packages
        if (isSystemUiPackage(packageName)) return

        // Check if this is actually a new foreground app
        val isNewApp = packageName != currentForegroundPackage || className != currentForegroundClass

        if (isNewApp) {
            val oldPackage = currentForegroundPackage
            currentForegroundPackage = packageName
            currentForegroundClass = className

            Log.d(TAG, "Foreground app changed: $oldPackage -> $packageName ($className)")

            // Broadcast locally for other components
            broadcastForegroundChange(packageName, className)

            // Notify Rust daemon via JNI
            notifyJni(packageName, className)
        }
    }

    private fun isSystemUiPackage(packageName: String): Boolean {
        return packageName.startsWith("android.") ||
               packageName.startsWith("com.android.systemui") ||
               packageName.startsWith("com.android.launcher") ||
               packageName.startsWith("com.google.android.apps.nexuslauncher") ||
               packageName == "com.android.keyguard"
    }

    private fun broadcastForegroundChange(packageName: String, className: String) {
        val intent = Intent(ACTION_FOREGROUND_APP_CHANGED).apply {
            setPackage(packageName) // restrict to our package
            putExtra(EXTRA_PACKAGE_NAME, packageName)
            putExtra(EXTRA_CLASS_NAME, className)
            putExtra(EXTRA_EVENT_TYPE, AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED)
        }
        sendBroadcast(intent)
    }

    private fun notifyJni(packageName: String, className: String) {
        sJniCallback?.onForegroundAppChanged(packageName, className)
    }

    override fun onInterrupt() {
        Log.w(TAG, "ForegroundDetectorService interrupted")
        currentForegroundPackage = null
        currentForegroundClass = null
    }

    override fun onDestroy() {
        super.onDestroy()
        Log.d(TAG, "ForegroundDetectorService destroyed")
        currentForegroundPackage = null
        currentForegroundClass = null
    }
}