/// Open a platform native "Save File" dialog.
library;

import 'dart:typed_data' show Uint8List;

import 'package:flutter_file_saver/flutter_file_saver.dart'
    show FileSaverCancelledException, FlutterFileSaver;
import 'package:lexeapp/result.dart';

class FilePath {
  const FilePath(this.path);

  final String path;

  @override
  String toString() => path;
}

/// Open a platform native "Save File" dialog to save a file with `filename`
/// and `data`.
///
/// Returns the path to the file if the user saved it, or `null` if the user
/// closed the dialog before saving.
///
/// Not supported on Linux or Windows.
Future<Result<FilePath?, Exception>> openDialog({
  required String filename,
  required Uint8List data,
}) async {
  final res = await Result.tryAsync<String, Exception>(
    () => FlutterFileSaver().writeFileAsBytes(fileName: filename, bytes: data),
  );
  switch (res) {
    case Ok(:final ok):
      return Ok(FilePath(ok));
    case Err(:final err):
      if (err is FileSaverCancelledException) {
        return const Ok(null);
      }
      return Err(err);
  }
}
