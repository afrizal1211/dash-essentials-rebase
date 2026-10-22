package com.redfox.daemon.jni

import android.content.Context
import android.util.Log

/**
 * JNI Bridge for ForegroundDetectorService to communicate with Rust daemon.
 *
 * Provides native methods to notify the Rust daemon of foreground app changes.
 */
class ForegroundDetectorJni {

    companion object {
        private const val TAG = "ForegroundDetectorJni"
        private var sLoaded = false

        @JvmStatic
        fun init(context: Context): Boolean {
            if (sLoaded) return true

            try {
                System.loadLibrary("dash_essential_host")
                sLoaded = true
                Log.d(TAG, "JNI library loaded successfully")
                return true
            } catch (e: UnsatisfiedLinkError) {
                Log.e(TAG, "Failed to load JNI library: ${e.message}")
                return false
            }
        }

        @JvmStatic
        fun isLoaded(): Boolean = sLoaded
    }

    /**
     * Notify the Rust daemon of a foreground app change.
     *
     * @param packageName The package name of the new foreground app
     * @param className The class name of the foreground activity
     * @return true if successful, false otherwise
     */
    @JvmStatic
    external fun notifyForegroundAppChanged(packageName: String, className: String): Boolean

    /**
     * Check if the JNI bridge is available.
     */
    @JvmStatic
    external fun isAvailable(): Boolean
}

object ForegroundDetectorJniHolder {
    @JvmStatic
    fun init(context: Context): Boolean = ForegroundDetectorJni.init(context)

    @JvmStatic
    fun notifyForegroundAppChanged(packageName: String, className: String): Boolean {
        return ForegroundDetectorJni.isLoaded() && ForegroundDetectorJni.notifyForegroundAppChanged(packageName, className)
    }

    @JvmStatic
    fun isAvailable(): Boolean = ForegroundDetectorJni.isLoaded() && ForegroundDetectorJni.isAvailable()
}