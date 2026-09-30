package com.opensurf.browser;

import android.Manifest;
import android.app.AlertDialog;
import android.net.Uri;
import android.webkit.GeolocationPermissions;
import android.webkit.PermissionRequest;

import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;

/**
 * Camera, microphone and location requests from websites. The user is always asked first
 * (naming the origin); only then are the matching Android runtime permissions requested.
 * Every request is answered exactly once; unsupported resources are denied.
 */
final class SitePermissions {
    /** State of one prompt, so it is resolved exactly once. */
    private static final class Prompt {
        boolean decided;
        boolean canceled;
        AlertDialog dialog;
    }

    private final MainActivity activity;
    private final Map<PermissionRequest, Prompt> mediaPrompts = new IdentityHashMap<>();
    private final List<Prompt> geolocationPrompts = new ArrayList<>();

    SitePermissions(MainActivity activity) {
        this.activity = activity;
    }

    void onPermissionRequest(PermissionRequest request) {
        boolean camera = false;
        boolean microphone = false;
        for (String resource : request.getResources()) {
            if (PermissionRequest.RESOURCE_VIDEO_CAPTURE.equals(resource)) {
                camera = true;
            } else if (PermissionRequest.RESOURCE_AUDIO_CAPTURE.equals(resource)) {
                microphone = true;
            }
        }
        if (!camera && !microphone) {
            request.deny(); // e.g. protected media or MIDI SysEx: not supported
            return;
        }
        final boolean wantsCamera = camera;
        final boolean wantsMicrophone = microphone;
        int message = camera && microphone ? R.string.perm_camera_microphone
                : camera ? R.string.perm_camera : R.string.perm_microphone;
        Prompt prompt = new Prompt();
        mediaPrompts.put(request, prompt);
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setMessage(activity.getString(message, describe(request.getOrigin())))
                .setPositiveButton(R.string.perm_allow, (d, w) -> {
                    prompt.decided = true;
                    grantMedia(request, prompt, wantsCamera, wantsMicrophone);
                })
                .setNegativeButton(R.string.perm_block, (d, w) -> {
                    prompt.decided = true;
                    mediaPrompts.remove(request);
                    request.deny();
                });
        prompt.dialog = activity.showDialog(builder, d -> {
            if (!prompt.decided) {
                prompt.decided = true;
                mediaPrompts.remove(request);
                if (!prompt.canceled) {
                    request.deny();
                }
            }
        });
    }

    private void grantMedia(PermissionRequest request, Prompt prompt, boolean camera,
            boolean microphone) {
        List<String> permissions = new ArrayList<>();
        if (camera) {
            permissions.add(Manifest.permission.CAMERA);
        }
        if (microphone) {
            permissions.add(Manifest.permission.RECORD_AUDIO);
        }
        activity.requestRuntimePermissions(permissions.toArray(new String[0]), () -> {
            mediaPrompts.remove(request);
            if (prompt.canceled) {
                return;
            }
            List<String> granted = new ArrayList<>();
            if (camera && activity.hasPermission(Manifest.permission.CAMERA)) {
                granted.add(PermissionRequest.RESOURCE_VIDEO_CAPTURE);
            }
            if (microphone && activity.hasPermission(Manifest.permission.RECORD_AUDIO)) {
                granted.add(PermissionRequest.RESOURCE_AUDIO_CAPTURE);
            }
            if (granted.isEmpty()) {
                request.deny();
            } else {
                request.grant(granted.toArray(new String[0]));
            }
        });
    }

    void onPermissionRequestCanceled(PermissionRequest request) {
        Prompt prompt = mediaPrompts.remove(request);
        if (prompt != null) {
            prompt.canceled = true;
            if (prompt.dialog != null) {
                prompt.dialog.dismiss();
            }
        }
    }

    void onGeolocationPrompt(String origin, GeolocationPermissions.Callback callback) {
        Prompt prompt = new Prompt();
        geolocationPrompts.add(prompt);
        AlertDialog.Builder builder = new AlertDialog.Builder(activity)
                .setMessage(activity.getString(R.string.perm_location, describe(Uri.parse(origin))))
                .setPositiveButton(R.string.perm_allow, (d, w) -> {
                    prompt.decided = true;
                    activity.requestRuntimePermissions(new String[] {
                            Manifest.permission.ACCESS_FINE_LOCATION,
                            Manifest.permission.ACCESS_COARSE_LOCATION}, () -> {
                                geolocationPrompts.remove(prompt);
                                if (!prompt.canceled) {
                                    boolean allowed =
                                            activity.hasPermission(Manifest.permission.ACCESS_FINE_LOCATION)
                                            || activity.hasPermission(Manifest.permission.ACCESS_COARSE_LOCATION);
                                    callback.invoke(origin, allowed, false);
                                }
                            });
                })
                .setNegativeButton(R.string.perm_block, (d, w) -> {
                    prompt.decided = true;
                    geolocationPrompts.remove(prompt);
                    callback.invoke(origin, false, false);
                });
        prompt.dialog = activity.showDialog(builder, d -> {
            if (!prompt.decided) {
                prompt.decided = true;
                geolocationPrompts.remove(prompt);
                if (!prompt.canceled) {
                    callback.invoke(origin, false, false);
                }
            }
        });
    }

    void onGeolocationPromptHidden() {
        for (Prompt prompt : new ArrayList<>(geolocationPrompts)) {
            prompt.canceled = true;
            if (prompt.dialog != null) {
                prompt.dialog.dismiss();
            }
        }
        geolocationPrompts.clear();
    }

    /** "https://example.com" style label for an origin. */
    private static String describe(Uri origin) {
        if (origin == null || origin.getHost() == null) {
            return String.valueOf(origin);
        }
        String label = origin.getScheme() + "://" + origin.getHost();
        return origin.getPort() > 0 ? label + ":" + origin.getPort() : label;
    }
}
