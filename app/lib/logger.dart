import 'dart:async' show Stream;

import 'package:app_rs_dart/ffi/ffi.dart' show initRustLogStream;
import 'package:flutter/foundation.dart' show debugPrint;
import 'package:lexeapp/cfg.dart' as cfg;
import 'package:lexeapp/result.dart';

const int _levelTrace = 0;
const int _levelDebug = 1;
const int _levelInfo = 2;
const int _levelWarn = 3;
const int _levelError = 4;

// The global logger instance. Might be uninitialized (null).
Logger? _logger;

/// The global logger state and configuration.
class Logger {
  const Logger._(this.minLogLevel);

  final int minLogLevel;

  /// Initialize the global logger. Will throw an exception if it's already
  /// initialized.
  static void init() {
    final alreadyInit = !tryInit();
    if (alreadyInit) {
      throw Exception("Dart logger is already initialized!");
    }
  }

  /// Try to initialize the global logger. Returns `true` if successful, `false`
  /// if the logger was already initialized.
  static bool tryInit() {
    final String rustLog;
    final int minLogLevel;

    // Check if the global logger is already set.
    if (_logger != null) {
      return false;
    } else {
      rustLog = cfg.rustLogFromEnv();
      minLogLevel = _logLevelFromRustLog(rustLog);
      _logger = Logger._(minLogLevel);
    }

    // Register a stream of log entries from Rust -> Dart.
    final Stream<String> rustLogRx;

    switch (Result.tryFfi(() => initRustLogStream(rustLog: rustLog))) {
      case Ok(:final ok):
        rustLogRx = ok;
      case Err():
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

  void log(int logLevel, String message) {
    if (logLevel >= this.minLogLevel) {
      final timestamp = DateTime.now().toUtc().microsecondsSinceEpoch * 1e-6;
      final levelString = _levelToString(logLevel);

      this.logRaw("${timestamp.toStringAsFixed(6)} D $levelString $message");
    }
  }

  @pragma('vm:prefer-inline')
  void logRaw(String formattedLog) {
    debugPrint(formattedLog);
  }
}

@pragma('vm:prefer-inline')
T dbg<T>(T value) {
  info("$value");
  return value;
}

@pragma('vm:prefer-inline')
void trace(String message) {
  _logger?.log(_levelTrace, message);
}

@pragma('vm:prefer-inline')
void debug(String message) {
  _logger?.log(_levelDebug, message);
}

@pragma('vm:prefer-inline')
void info(String message) {
  _logger?.log(_levelInfo, message);
}

@pragma('vm:prefer-inline')
void warn(String message) {
  _logger?.log(_levelWarn, message);
}

@pragma('vm:prefer-inline')
void error(String message) {
  _logger?.log(_levelError, message);
}

@pragma('vm:prefer-inline')
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

@pragma('vm:prefer-inline')
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
