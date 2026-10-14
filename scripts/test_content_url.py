"""Source recognition and train-only installs use no real network."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import download_official_content as content
from test_official_content import Response, archive

class UrlContent(unittest.TestCase):
    def test_catalogue_aliases_and_repository_root_are_canonical(self):
        base='https://github.com/DocMartin7644/Chiltern-Route-v4'
        for suffix in ('', '/', '.git'):
            source=content.source_from_url(base+suffix)
            self.assertEqual(source['id'],'chiltern-v4')
            self.assertFalse(source['requires_confirmation'])
        source=content.source_from_url(base+'/archive/refs/heads/main.zip')
        self.assertEqual(source['requested_ref'],'main')
        self.assertFalse(source['requires_confirmation'])

    def test_new_author_requires_explicit_origin_confirmation(self):
        source=content.source_from_url('https://github.com/OwnAuthor/OwnRoute')
        self.assertTrue(source['requires_confirmation'])
        self.assertEqual(source['author']['name'],'OwnAuthor')
        source=content.source_from_url(source['url'],confirmed=True)
        self.assertTrue(source['original_confirmed'])
        self.assertFalse(source['requires_confirmation'])

    def test_user_origin_identity_cannot_collide_with_catalogue_or_other_owner(self):
        urls = ['https://github.com/demo/model-1', 'https://github.com/a-b/c',
                'https://github.com/a/b-c']
        ids = [content.source_from_url(url, confirmed=True)['id'] for url in urls]
        self.assertEqual(len(set(ids)), 3)
        self.assertTrue(all(value.startswith('url-github-') for value in ids))
        self.assertTrue(set(ids).isdisjoint(entry['id'] for entry in content.catalog()['routes']))
        canonical = content.source_from_url(urls[1])['id']
        for url in ['https://github.com/A-B/C.git', urls[1]+'/',
                    urls[1]+'/archive/refs/heads/main.zip']:
            self.assertEqual(content.source_from_url(url, confirmed=True)['id'], canonical)

    def test_credentials_local_addresses_and_landing_pages_are_rejected(self):
        urls=['http://github.com/author/route','file:///tmp/payload.zip',
              'https://localhost/x.zip','https://127.0.0.1/x.zip',
              'https://github.com@evil.test/a/b', 'https://u:p@github.com/a/b',
              'https://github.com:8443/a/b','https://github.com/a/b?token=secret',
              'https://github.com/a/b/tree/main', 'https://api.github.com/a/b',
              'https://github.com/../..', 'https://other-author.test/content.zip']
        for url in urls:
            with self.subTest(url=url),self.assertRaises(ValueError): content.source_from_url(url)

    def test_cli_inspection_reports_connection_failure_without_traceback(self):
        import contextlib, io
        stderr = io.StringIO()
        with patch('sys.argv', ['download_official_content.py', '--inspect-url', '--url', 'https://github.com/OwnAuthor/OwnRoute']), \
             patch.object(content, 'https_open', side_effect=OSError('connection unavailable')), \
             contextlib.redirect_stderr(stderr), self.assertRaises(SystemExit) as stopped:
            content.main()
        self.assertEqual(stopped.exception.code, 2)
        self.assertIn('Revisá el enlace público y la conexión', stderr.getvalue())
        self.assertNotIn('Traceback', stderr.getvalue())

    def test_vehicle_only_archive_preserves_author_files_and_requires_audit(self):
        with tempfile.TemporaryDirectory() as tmp:
            source=content.source_from_url('https://github.com/OwnAuthor/TrainOnly',confirmed=True)
            payload=archive([('TrainOnly/TRAINSET/Train/power.eng','engine original'),
                             ('TrainOnly/LICENSE','author licence')])
            revision=dict(repository='OwnAuthor/TrainOnly',commit='f'*40)
            with patch.object(content,'resolve_download',return_value=('https://codeload.github.com/OwnAuthor/TrainOnly/zip/'+'f'*40,revision)),patch.object(content,'emit'):
                installed=content.install(source,Path(tmp),opener=lambda _:Response(payload))
            metadata=json.loads((installed/'openrailsrs-content.json').read_text())
            self.assertEqual(metadata['routes'],[])
            self.assertEqual(metadata['stock_files'],['TrainOnly/TRAINSET/Train/power.eng'])
            self.assertEqual(metadata['origin_verification'],'player_confirmed')
            self.assertEqual((installed/'TrainOnly/LICENSE').read_text(),'author licence')
            self.assertIn('pending',metadata['audit'])

    def test_an_archive_without_railway_content_does_not_become_installed(self):
        with tempfile.TemporaryDirectory() as tmp:
            source=content.source_from_url('https://github.com/OwnAuthor/NoAssets',confirmed=True)
            with patch.object(content,'resolve_download',return_value=('https://codeload.github.com/OwnAuthor/NoAssets/zip/'+'f'*40,dict(repository='OwnAuthor/NoAssets',commit='f'*40))),patch.object(content,'emit'),self.assertRaises(ValueError):
                content.install(source,Path(tmp),opener=lambda _:Response(archive([('x/readme','text')])))
            self.assertEqual(list(Path(tmp).iterdir()),[])

    def test_inspection_resolves_revision_without_fetching_the_archive(self):
        source = content.source_from_url('https://github.com/OwnAuthor/OwnRoute')
        metadata = dict(full_name='OwnAuthor/OwnRoute', default_branch='main', pushed_at='2026-10-13T00:00:00Z')
        commit = dict(sha='a'*40, commit=dict(committer=dict(date='2026-10-12T12:00:00Z')))
        with patch.object(content, 'https_open', side_effect=[Response(json.dumps(metadata).encode()), Response(json.dumps(commit).encode())]) as request:
            preview = content.inspect_source(source)
        self.assertEqual(request.call_count, 2)
        self.assertTrue(all(call.args[0].startswith('https://api.github.com/') for call in request.call_args_list))
        self.assertEqual(preview['resolved_commit'], 'a'*40)
        self.assertEqual(preview['package_type'], 'repository')
        self.assertTrue(preview['requires_confirmation'])

    def test_redirects_cannot_switch_author_or_repository_before_download(self):
        original = 'https://github.com/OwnAuthor/OwnRoute/archive/main.zip'
        for redirected in ['https://codeload.github.com/Another/OwnRoute/zip/main',
                           'https://codeload.github.com/OwnAuthor/Another/zip/main',
                           'https://static.openrails.org/files/DemoModel1.zip']:
            with self.subTest(url=redirected), self.assertRaisesRegex(ValueError, 'repositorio'):
                content.validate_redirect(original, redirected)
        content.validate_redirect(original, 'https://codeload.github.com/OwnAuthor/OwnRoute/zip/main')
        content.validate_redirect('https://api.github.com/repos/DocMartin7644/Chiltern-Route-v2',
                                  'https://api.github.com/repos/DocMartin7644/Chiltern-Route-v4')

    def test_repository_transfer_requires_a_new_author_confirmation(self):
        source = content.source_from_url('https://github.com/OwnAuthor/OwnRoute', confirmed=True)
        metadata = dict(full_name='NewOwner/OwnRoute', default_branch='main')
        with patch.object(content, 'https_open', return_value=Response(json.dumps(metadata).encode())), \
             self.assertRaisesRegex(ValueError, 'cambió de origen'):
            content.resolve_download(source)

    def test_mid_download_failure_and_cancel_leave_previous_version_untouched(self):
        source = content.source_from_url('https://github.com/OwnAuthor/OwnRoute', confirmed=True)
        revision = dict(repository='OwnAuthor/OwnRoute', commit='f'*40)
        payload = archive([('Route/ROUTES/R/R.trk', 'route'), ('Route/ROUTES/R/WORLD/t.w', 'world')])
        for cancellation in (False, True):
            with self.subTest(cancel=cancellation), tempfile.TemporaryDirectory() as tmp:
                destination = Path(tmp)
                old = destination / 'original-installed'
                old.mkdir(); (old/'sentinel').write_text('original')
                cancel = destination/'cancel'
                class Partial(Response):
                    def read(self, size=-1):
                        if self.tell():
                            if cancellation:
                                cancel.touch()
                                return b'x'
                            raise OSError('network connection lost')
                        return super().read(16)
                with patch.object(content, 'resolve_download', return_value=(
                         'https://codeload.github.com/OwnAuthor/OwnRoute/zip/'+'f'*40, revision)), \
                     patch.object(content, 'emit'), self.assertRaises((InterruptedError, OSError)):
                    content.install(source, destination, cancel, opener=lambda _: Partial(payload))
                self.assertEqual((old/'sentinel').read_text(), 'original')
                self.assertEqual(sorted(p.name for p in destination.iterdir()),
                                 ['cancel', 'original-installed'] if cancellation else ['original-installed'])

    def test_large_vehicle_inventory_still_has_a_bounded_completion_event(self):
        import contextlib, io
        manifest = {'stock_files': [f'Original/TRAINSET/Original-Train/vehicle-{i}.eng' for i in range(7000)], 'routes': []}
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            content.emit('complete', path='/tmp/original', manifest=manifest)
        line = output.getvalue()
        self.assertLess(len(line.encode()), 64*1024)
        event = json.loads(line)
        self.assertEqual(event['manifest']['stock_files_count'], 7000)
        self.assertEqual(len(manifest['stock_files']), 7000)
        self.assertEqual(event['manifest_path'], '/tmp/original/openrailsrs-content.json')

    def test_low_space_does_not_start_a_download(self):
        source = content.source_from_url('https://github.com/OwnAuthor/OwnRoute', confirmed=True)
        with tempfile.TemporaryDirectory() as tmp, \
             patch.object(content.shutil, 'disk_usage', return_value=content.shutil._ntuple_diskusage(1024, 1023, 1)), \
             self.assertRaisesRegex(ValueError, 'espacio'):
            content.install(source, Path(tmp), opener=lambda _: self.fail('No network if disk is full'))
        self.assertFalse(any(Path(tmp).glob('*')))

if __name__=='__main__': unittest.main()
