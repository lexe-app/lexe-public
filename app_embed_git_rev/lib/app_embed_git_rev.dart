import 'dart:io';

Future<String> getGitRevision() async {
  try {
    // Check if working tree is dirty
    final statusResult = await Process.run('git', ['status', '--porcelain']);
    if (statusResult.exitCode != 0 ||
        statusResult.stdout.toString().trim().isNotEmpty) {
      return 'dirty';
    }

    // Try to get the revision of the subrepo/lexe-public branch
    final revResult = await Process.run('git', [
      'rev-parse',
      'subrepo/lexe-public',
    ]);
    if (revResult.exitCode != 0) {
      return 'dirty';
    }

    return revResult.stdout.toString().trim();
  } catch (e) {
    return 'dirty';
  }
}
