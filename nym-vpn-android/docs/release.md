## Release

This doc describes how to release a new version of the NymVPN
Android app.

### Types of releases

Release tags must follow the following patterns:

- **stable** `nym-vpn-android-v1.2.3`
- dev `nym-vpn-android-v1.2.3-dev`
- RC `nym-vpn-android-v1.2.3-rc.1`
- nightly `nym-vpn-android-nightly`

> **_NOTE:_**  Nightly releases are triggered on via a scheduled cron job and build from the the `develop` branch.

### Bump versions

> **_IMPORTANT:_**  Version changes should never land in develop unless being cherry-picked from a release branch after a release.
>
> This is because a version code/name change will trigger a Fdroid automatic update. We only want this to occur after a successful release from a release branch.

1. Navigate to the file `buildSrc/src/main/kotlin/Constants.kt` \
   and update the version name and version code.
   ```kotlin
   const val VERSION_NAME = "v1.2.3"
   const val VERSION_CODE = 12300
	```
   * first three digits of version code should match the version name
   * last two digits used for different builds types (prerelease builds)

2. If releasing a **stable** release, a release notes file must be \
   created in `fastlane/metadata/android/en-US/changelogs/12300.txt.` \
   Following existing release note files for typical formatting. If not \
   creating a **stable** release, skip this step.

3. Push the changes to the repository on a branch.

### Releasing the app

Go to the workflow
   [publish-nym-vpn-android](https://github.com/nymtech/nym-vpn-client/actions/workflows/publish-nym-vpn-android.yml)
   and click on the _Run workflow_ button

> **_NOTE:_** When creating a **stable** GitHub release, this will automatically publish to [Nym's F-Droid](https://github.com/nymtech/fdroid) repository \
by matching against the GitHub release tag name and to  the [F-Droid official](https://f-droid.org/) repository by matching against \
version code/name/tag. [F-Droid official](https://f-droid.org/) takes roughly one week for the new version to be available in the store. \
Progress can be monitored using the [F-Droid monitor](https://monitor.f-droid.org/builds). [Nym's F-Droid](https://github.com/nymtech/fdroid) should have \
the new version available for download almost immediately when the version is published to GitHub.

1. Select the branch from which the release should be made \
   (including the version bump changes)

2. Select the Google Play Store release track for this build
   * _production_ releases to all play users (stable)
   * _beta_ releases to beta tester play users
   * _alpha_ release to alpha tester (email registered) play users
   * _internal_ releases to internal testers (the team)
   * _none_ skips store release

	> All Google Play releases must pass the their review process. The current status of the \
	review process can be monitored in the [Google Play Console](https://play.google.com/console/).

3. `Skip app bundle` checkbox only applies if a Google Play release \
track other than _none_ has been selected. This should be checked \
when **only publishing metadata file changes** (screenshots, app descriptions, \
localizations, etc) and not publishing a new application version.

4. `Skip app metadata` checkbox also only applies if a Google Play release \
track other than _none_ has been selected. This should be checked to skip \
publishing app metadata to Google Play. This is useful when there are errors with \
missing localization files or screenshots that are blocking a deployment or when \
publishing a preproduction app version.

5. `Publish accrescent` checkbox would typically only apply on **stable** releases. \
Checking this box will trigger a job on release to create a `.apks` file from the release \
bundle which is the file type the [Accrescent](https://accrescent.app/) store expects. This \
file can then needs to be downloaded from the workflow and uploaded to the \
[Accrescent console](https://console.accrescent.app/login). If you do not already have access, \
access can be gained by emailing the Accrescent developer and requesting access.

6. Enter in `Tag name for release` the tag name following the release tag naming \
convention highlighted above.

7. Select in `GitHub release type` the release type for the release to GitHub \
   * _release_ is for a **stable** releases
   * _prerelease_ is for any dev or RC release
   * _nightly_ is for triggering a nightly release manually
   * _none_ skips GitHub release

8. When everything is correct, click _Run workflow_.


