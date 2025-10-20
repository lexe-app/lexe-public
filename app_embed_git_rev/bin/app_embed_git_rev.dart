import 'dart:io';
import 'package:app_embed_git_rev/app_embed_git_rev.dart';
import 'package:args/args.dart';

/// This tiny CLI is run by our flutter `app` during build to embed the public
/// repo git revision into the built app bundle so that we know whether it's
/// fresh.
Future<void> main(List<String> arguments) async {
  final parser = ArgParser()
    ..addOption('input', help: 'Input file path')
    ..addOption('output', help: 'Output file path', mandatory: true);

  try {
    final results = parser.parse(arguments);
    final outputPath = results['output'] as String;

    // Get the git revision
    final gitRevision = await getGitRevision();

    // Write the git revision to the --output file
    final outputFile = File(outputPath);
    await outputFile.writeAsString(gitRevision);
  } catch (e) {
    stderr.writeln('Error: $e');
    exit(1);
  }
}
