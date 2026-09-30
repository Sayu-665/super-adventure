# OpenSurf uses only the Android framework: no reflection, no JavaScript interfaces and no
# serialization, so the default optimized rules are sufficient. Activities, the WebView client
# subclasses (framework overrides) and resources referenced from XML are kept automatically.

# Keep line numbers readable in the (local-only) stack traces of release builds.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
