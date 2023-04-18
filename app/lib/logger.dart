import 'dart:async' show Stream;

import 'package:flutter/foundation.dart' show debugPrint;
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart' show FfiException;

import 'bindings.dart' show api;
import 'bindings_generated_api.dart' show LogEntry;

const int _levelTrace = 0;
const int _levelDebug = 1;
const int _levelInfo = 2;
const int _levelWarn = 3;
const int _levelError = 4;

// The global logger instance. Might be uninitialized (null).
_Logger? _logger;

/// The global logger state and configuration.
class _Logger {
  const _Logger(this.minLogLevel);

  final int minLogLevel;

  void log(int logLevel, String message) {
    if (logLevel >= this.minLogLevel) {
      final levelString = _levelToString(logLevel);
      this.logRaw("$levelString $message");
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
  // TODO(phlip9): make log level configurable
  const minLogLevel = _levelInfo;

  if (_logger != null) {
    return false;
  } else {
    _logger = const _Logger(minLogLevel);
  }

  // Register a stream of log entries from Rust -> Dart.
  final Stream<LogEntry> rustLogRx;

  try {
    rustLogRx = api.initRustLogStream();
  } on FfiException {
    // Rust logger is already initialized?
    return false;
  }

  // "Spawn" a task listening on the Rust log stream. It just forwards Rust log
  // messages to the Dart logger.
  //
  // For some reason, this doesn't spawn in a reasonable amount of time if the
  // tryInit fn isn't marked `async`...
  rustLogRx.listen((logEntry) {
    // Rust log messages are already filtered and formatted. Just print them
    // directly.
    _logger!.logRaw(logEntry.message);
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
