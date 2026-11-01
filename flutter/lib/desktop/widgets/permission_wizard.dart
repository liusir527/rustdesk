import 'dart:async';

import 'package:flutter/material.dart';
import 'package:get/get.dart';

import '../../common.dart';
import '../../models/platform_model.dart';

const kOptionPermissionWizardDismissed = "permission-wizard-dismissed";

enum _PermissionStatus { granted, missing }

class _PermissionItem {
  final String name;
  final Future<void> Function() request;
  _PermissionStatus status;

  _PermissionItem({
    required this.name,
    required this.request,
    this.status = _PermissionStatus.missing,
  });
}

/// First-run helper: the macOS permissions a controlled desktop needs,
/// listed in one place with live status, instead of surprising the user one
/// banner / system dialog at a time. Closes itself once everything is
/// granted; "Skip" hides it for this machine.
Future<void> maybeShowPermissionWizard(BuildContext context) async {
  if (!isMacOS || isWeb) return;
  if (bind.isIncomingOnly()) return;
  if (bind.mainGetLocalOption(key: kOptionPermissionWizardDismissed) == 'Y') {
    return;
  }

  final items = [
    _PermissionItem(
      name: 'Screen Recording',
      request: () async {
        await bind.mainIsCanScreenRecording(prompt: true);
      },
    ),
    _PermissionItem(
      name: 'Accessibility',
      request: () async {
        await bind.mainIsProcessTrusted(prompt: true);
      },
    ),
    _PermissionItem(
      name: 'Input Monitoring',
      request: () async {
        await bind.mainIsCanInputMonitoring(prompt: true);
      },
    ),
  ];

  bool refreshAll() {
    var allGranted = true;
    items[0].status = bind.mainIsCanScreenRecording(prompt: false)
        ? _PermissionStatus.granted
        : _PermissionStatus.missing;
    items[1].status = bind.mainIsProcessTrusted(prompt: false)
        ? _PermissionStatus.granted
        : _PermissionStatus.missing;
    items[2].status = bind.mainIsCanInputMonitoring(prompt: false)
        ? _PermissionStatus.granted
        : _PermissionStatus.missing;
    for (final item in items) {
      if (item.status != _PermissionStatus.granted) {
        allGranted = false;
      }
    }
    return allGranted;
  }

  if (refreshAll()) {
    return;
  }

  await gFFI.dialogManager.show<bool>((setState, close, context) {
    dismiss() {
      bind.mainSetLocalOption(key: kOptionPermissionWizardDismissed, value: 'Y');
      close(false);
    }

    return CustomAlertDialog(
      title: Text(translate('Permissions')),
      content: _PermissionWizardBody(
        items: items,
        refresh: refreshAll,
        onAllGranted: () {
          bind.mainSetLocalOption(
              key: kOptionPermissionWizardDismissed, value: 'Y');
          close(true);
        },
      ),
      actions: [
        dialogButton('Skip', onPressed: dismiss, isOutline: true),
      ],
      onCancel: dismiss,
    );
  });
}

class _PermissionWizardBody extends StatefulWidget {
  final List<_PermissionItem> items;
  final bool Function() refresh;
  final VoidCallback onAllGranted;

  const _PermissionWizardBody({
    required this.items,
    required this.refresh,
    required this.onAllGranted,
  });

  @override
  State<_PermissionWizardBody> createState() => _PermissionWizardBodyState();
}

class _PermissionWizardBodyState extends State<_PermissionWizardBody> {
  Timer? _timer;

  @override
  void initState() {
    super.initState();
    // Keep checking so a permission granted in System Settings clears its
    // row live, and the dialog closes itself when everything is granted.
    _timer = Timer.periodic(const Duration(seconds: 1), (timer) async {
      final allGranted = widget.refresh();
      if (!mounted) {
        timer.cancel();
        return;
      }
      setState(() {});
      if (allGranted) {
        timer.cancel();
        widget.onAllGranted();
      }
    });
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          translate('Permission wizard tip'),
          style: TextStyle(
              fontSize: 12,
              color: Theme.of(context)
                  .textTheme
                  .titleLarge
                  ?.color
                  ?.withOpacity(0.6)),
        ).marginOnly(bottom: 10),
        for (final item in widget.items)
          Row(
            children: [
              Icon(
                item.status == _PermissionStatus.granted
                    ? Icons.check_circle_rounded
                    : Icons.error_outline_rounded,
                size: 18,
                color: item.status == _PermissionStatus.granted
                    ? Colors.green
                    : Colors.orange,
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  translate(item.name),
                  style: const TextStyle(fontSize: 13),
                ),
              ),
              if (item.status != _PermissionStatus.granted)
                TextButton(
                  onPressed: () => item.request(),
                  child: Text(translate('Configure')),
                ),
            ],
          ).marginOnly(bottom: 4),
      ],
    );
  }
}
