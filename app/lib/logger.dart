import 'dart:async' show Stream;
import 'dart:io';

import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart' show FfiException;

import 'bindings.dart' show api;

const int _levelTrace = 0;
const int _levelDebug = 1;
const int _levelInfo = 2;
const int _levelWarn = 3;
const int _levelError = 4;

// The global logger instance. Might be uninitialized (null).
_Logger? _logger;

/// The global logger state and configuration.
class _Logger {
  _Logger(this.minLogLevel);

  final int minLogLevel;

  void log(int logLevel, String message) {
    if (logLevel >= this.minLogLevel) {
      final timestamp = DateTime.now().toUtc().microsecondsSinceEpoch * 1e-6;
      final levelString = _levelToString(logLevel);

      this.logRaw("${timestamp.toStringAsFixed(6)} D $levelString $message");
    }
  }

  void logRaw(String formattedLog) {
    debugPrint(formattedLog);
  }
}

/// Initialize the global logger. Will throw an exception if it's already
/// initialized.
void init() {
  final alreadyInit = !tryInit();
  if (alreadyInit) {
    throw Exception("Dart logger is already initialized!");
  }
}

/// Try to initialize the global logger. Returns `true` if successful, `false`
/// if the logger was already initialized.
bool tryInit() {
  final String rustLog;
  final int minLogLevel;

  // Check if the global logger is already set.
  if (_logger != null) {
    return false;
  } else {
    rustLog = _rustLogFromEnv();
    minLogLevel = _logLevelFromRustLog(rustLog);
    _logger = _Logger(minLogLevel);
  }

  // Register a stream of log entries from Rust -> Dart.
  final Stream<String> rustLogRx;

  try {
    rustLogRx = api.initRustLogStream(rustLog: rustLog);
  } on FfiException {
    // Rust logger is already initialized?
    return false;
  }

  // "Spawn" a task listening on the Rust log stream. It just forwards Rust log
  // messages to the Dart logger.
  //
  // For some reason, this doesn't spawn in a reasonable amount of time if the
  // tryInit fn isn't marked `async`...
  rustLogRx.listen((formattedLog) {
    // Rust log messages are already filtered and formatted. Just print them
    // directly.
    _logger!.logRaw(formattedLog);
  });

  return true;
}

void trace(String message) {
  _logger?.log(_levelTrace, message);
}

void debug(String message) {
  _logger?.log(_levelDebug, message);
}

void info(String message) {
  _logger?.log(_levelInfo, message);
}

void warn(String message) {
  _logger?.log(_levelWarn, message);
}

void error(String message) {
  _logger?.log(_levelError, message);
}

// Load the log filter from the environment. Priority:
// 1. env: $RUST_LOG (not available on mobile!!)
// 2. build-time: `flutter run --dart-define=RUST_LOG=$RUST_LOG ..`
//    (for `String.fromEnvironment`, for mobile)
// 3. default: INFO
String _rustLogFromEnv() {
  final String? envRustLog = Platform.environment["RUST_LOG"];

  if (envRustLog != null) {
    return envRustLog;
  }

  // this must be a separate const variable
  const String? buildTimeRustLog = bool.hasEnvironment("RUST_LOG")
      ? String.fromEnvironment("RUST_LOG")
      : null;

  if (buildTimeRustLog != null) {
    return buildTimeRustLog;
  }

  return "info";
}

String _levelToString(int logLevel) {
  if (logLevel == _levelTrace) {
    return "TRACE";
  } else if (logLevel == _levelDebug) {
    return "DEBUG";
  } else if (logLevel == _levelInfo) {
    return " INFO";
  } else if (logLevel == _levelWarn) {
    return " WARN";
  } else {
    return "ERROR";
  }
}

int? _parseLogLevel(String target) {
  if (target == "trace") {
    return _levelTrace;
  } else if (target == "debug") {
    return _levelDebug;
  } else if (target == "info") {
    return _levelInfo;
  } else if (target == "warn") {
    return _levelWarn;
  } else if (target == "error") {
    return _levelError;
  } else {
    return null;
  }
}

/// Dart loggers don't support getting the current module / target, so we can
/// only filter logs coarsely by log level. This fn parses out the first plain
/// log level (e.g. "info", "trace") and returns it as a log level int, or
/// `_levelInfo` by default.
///
/// ## Examples
///
/// ```dart
/// assert(_logLevelFromRustLog("") == _levelInfo);
/// assert(_logLevelFromRustLog("warn,sqlx=error") == _levelWarn);
/// assert(_logLevelFromRustLog("debug,sqlx=error,trace") == _levelDebug);
/// assert(_logLevelFromRustLog("asdf") == _levelInfo);
/// ```
int _logLevelFromRustLog(String rustLog) {
  final targets = rustLog.split(',');
  for (final target in targets) {
    final logLevel = _parseLogLevel(target);
    if (logLevel != null) {
      return logLevel;
    }
  }
  return _levelInfo;
}
