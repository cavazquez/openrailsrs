#!/usr/bin/env python3
"""Offline installer invariants: atomicity, traversal, symlinks, cancellation."""
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.request import BaseHandler, build_opener
from urllib.response import addinfourl
from email.message import Message
import zipfile
import download_official_content as content


class Response(io.BytesIO):
    def __init__(self, data):
        super().__init__(data)
        self.headers = {'Content-Length': str(len(data))}


def archive(files):
    data = io.BytesIO()
    with zipfile.ZipFile(data, 'w') as z:
        for name, value in files:
            z.writestr(name, value)
    return data.getvalue()


class Installer(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.entry = next(e for e in content.catalog()['routes'] if e['id'] == 'demo-model-1')

    def tearDown(self):
        self.temp.cleanup()

    def test_complete_package_is_atomic_and_has_origin_and_digest(self):
        data = archive([('Demo/ROUTES/Route/Route.trk', 'route'),
                        ('Demo/ROUTES/Route/WORLD/tile.w', 'world'),
                        ('Demo/author-license.txt', 'licence')])
        with patch.object(content, 'emit'):
            target = content.install(self.entry, self.root, opener=lambda _: Response(data))
            before = target.joinpath('Demo/author-license.txt').read_text()
            reused = content.install(self.entry, self.root, opener=lambda _: Response(data))
        self.assertEqual(target, reused)
        m = json.loads((target / 'openrailsrs-content.json').read_text())
        self.assertEqual(m['download_sha256'], content.hashlib.sha256(data).hexdigest())
        self.assertEqual(m['routes'], ['Demo/ROUTES/Route'])
        self.assertEqual(before, 'licence')
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), [target.name])

    def test_latest_author_revision_is_checked_and_old_and_new_copies_coexist(self):
        entry=self.entry | {'url':'https://github.com/author/route.git'}
        first=dict(repository='author/route',commit='1'*40,published_at='2026-01-01T00:00:00Z')
        second=dict(repository='author/route',commit='2'*40,published_at='2026-10-05T00:00:00Z')
        payload=lambda text:archive([('Demo/ROUTES/Route/Route.trk',text),('Demo/ROUTES/Route/WORLD/tile.w','world')])
        with patch.object(content,'emit'),patch.object(content,'resolve_download',side_effect=[('https://codeload.github.com/author/route/zip/1',first),('https://codeload.github.com/author/route/zip/2',second),('https://codeload.github.com/author/route/zip/2',second)]):
            old=content.install(entry,self.root,opener=lambda _:Response(payload('old')))
            new=content.install(entry,self.root,opener=lambda _:Response(payload('new')))
            reused=content.install(entry,self.root,opener=lambda _:self.fail('Current revision must reuse content after checking the author'))
        self.assertNotEqual(old,new); self.assertEqual(new,reused)
        self.assertEqual((old/'Demo/ROUTES/Route/Route.trk').read_text(),'old')
        metadata=json.loads((new/'openrailsrs-content.json').read_text())
        self.assertEqual(metadata['source_date'],'2026-10-05T00:00:00Z')
        self.assertIn('downloaded_at',metadata)

    def test_zip_entity_change_preserves_previous_copy_and_same_entity_skips_body(self):
        def response(text,tag):
            r=Response(archive([('Demo/ROUTES/Route/Route.trk',text),('Demo/ROUTES/Route/WORLD/tile.w','world')]))
            r.headers.update({'ETag':tag,'Last-Modified':'Mon, 05 Oct 2026 12:00:00 GMT'})
            return r
        with patch.object(content,'emit'):
            old=content.install(self.entry,self.root,opener=lambda _:response('old','one'))
            new=content.install(self.entry,self.root,opener=lambda _:response('new','two'))
            latest=response('never read','two')
            latest.read=lambda _:self.fail('Unchanged entity must not download the archive body')
            reused=content.install(self.entry,self.root,opener=lambda _:latest)
        self.assertNotEqual(old,new); self.assertEqual(new,reused)
        self.assertTrue(json.loads((new/'openrailsrs-content.json').read_text())['source_date'].startswith('2026-10-05T12:00:00'))

    def test_default_storage_is_user_owned_and_snap_revision_independent(self):
        self.assertEqual(content.player_data_dir({'HOME':'/home/test'}),Path('/home/test/.local/share/openrailsrs'))
        self.assertEqual(content.player_data_dir({'SNAP_USER_COMMON':'/home/test/snap/app/common','SNAP_USER_DATA':'/home/test/snap/app/7'}),Path('/home/test/snap/app/common/openrailsrs'))
        self.assertEqual(content.player_data_dir({'HOME':'/home/test','XDG_DATA_HOME':'bad-relative'}),Path('/home/test/.local/share/openrailsrs'))

    def test_chiltern_v4_has_a_distinct_original_source_and_preserves_v2(self):
        entries = {entry['id']: entry for entry in content.catalog()['routes']}
        new = entries['chiltern-v4']
        old = new['previousSources'][0] | {'compensation': 'free', 'author': new['author']}
        self.assertNotIn('chiltern', entries, 'The old repository now redirects to v4')
        self.assertEqual(old['url'], 'https://github.com/DocMartin7644/Chiltern-Route-v2.git')
        self.assertEqual(new['url'], 'https://github.com/DocMartin7644/Chiltern-Route-v4.git')
        self.assertEqual(old['author'], new['author'])
        self.assertEqual(content.provider(new), 'github')
        payload = archive([('Chiltern/ROUTES/Chiltern/Chiltern.trk', 'route'),
                           ('Chiltern/ROUTES/Chiltern/WORLD/tile.w', 'world'),
                           ('Chiltern/LICENSE.txt', 'original licence')])

        def resolve(entry):
            repository = entry['url'].removeprefix('https://github.com/').removesuffix('.git')
            return 'https://codeload.github.com/' + repository + '/zip/' + '1' * 40, {
                'repository': repository, 'commit': '1' * 40}

        with patch.object(content, 'emit'), patch.object(content, 'resolve_download', side_effect=resolve):
            previous = content.install(old | {'installSize': 0}, self.root, opener=lambda _: Response(payload))
            current = content.install(new | {'installSize': 0}, self.root, opener=lambda _: Response(payload))
            reused = content.install(new | {'installSize': 0}, self.root,
                                     opener=lambda _: self.fail('The installed v4 revision must be reused'))
        self.assertNotEqual(previous, current)
        self.assertEqual(current, reused)
        self.assertTrue(previous.name.startswith('chiltern-'))
        self.assertTrue(current.name.startswith('chiltern-v4-'))
        self.assertEqual((previous / 'Chiltern/LICENSE.txt').read_text(), 'original licence')
        manifest = json.loads((current / 'openrailsrs-content.json').read_text())
        self.assertEqual(manifest['catalog_source'], new['source_url'])
        self.assertEqual(manifest['advertised_url'], new['url'])

    def test_renamed_repository_reuses_legacy_copy_only_for_the_same_current_commit(self):
        new = next(entry for entry in content.catalog()['routes'] if entry['id'] == 'chiltern-v4')
        old = new['previousSources'][0] | {'compensation': 'free', 'author': new['author'], 'installSize': 0}
        current = dict(repository='DocMartin7644/Chiltern-Route-v4', commit='1' * 40)
        updated = current | {'commit': '2' * 40}
        payload = lambda text: archive([('Chiltern/ROUTES/Chiltern/Chiltern.trk', text),
                                        ('Chiltern/ROUTES/Chiltern/WORLD/tile.w', 'world')])
        with patch.object(content, 'emit'), patch.object(content, 'resolve_download', side_effect=[
                ('https://codeload.github.com/original/zip/1', current),
                ('https://codeload.github.com/original/zip/1', current),
                ('https://codeload.github.com/original/zip/2', updated)]):
            previous = content.install(old, self.root, opener=lambda _: Response(payload('old')))
            reused = content.install(new | {'installSize': 0}, self.root,
                                     opener=lambda _: self.fail('A renamed origin with the same commit must reuse the legacy copy'))
            latest = content.install(new | {'installSize': 0}, self.root, opener=lambda _: Response(payload('new')))
        self.assertEqual(previous, reused)
        self.assertNotEqual(previous, latest)
        self.assertEqual((previous / 'Chiltern/ROUTES/Chiltern/Chiltern.trk').read_text(), 'old')
        self.assertEqual((latest / 'Chiltern/ROUTES/Chiltern/Chiltern.trk').read_text(), 'new')

    def test_legacy_cli_package_alias_uses_the_canonical_author_entry(self):
        with patch('sys.argv', ['download_official_content.py', '--package', 'chiltern']), \
                patch.object(content, 'install') as install:
            content.main()
        self.assertEqual(install.call_args.args[0]['id'], 'chiltern-v4')
        self.assertEqual(install.call_args.args[0]['url'], 'https://github.com/DocMartin7644/Chiltern-Route-v4.git')

    def test_malformed_cached_metadata_does_not_prevent_a_fresh_install(self):
        bad=self.root/(self.entry['id']+'-malformed')
        bad.mkdir(); (bad/'openrailsrs-content.json').write_text('[]')
        payload=archive([('Demo/ROUTES/Route/Route.trk','route'),('Demo/ROUTES/Route/WORLD/tile.w','world')])
        with patch.object(content,'emit'):
            target=content.install(self.entry,self.root,opener=lambda _:Response(payload))
        self.assertNotEqual(target,bad)
        self.assertTrue((target/'openrailsrs-content.json').is_file())

    @unittest.skipUnless(content.os.name == 'posix', 'symlink regression')
    def test_existing_digest_symlink_is_never_reused(self):
        payload=archive([('Demo/ROUTES/Route/Route.trk','route'),('Demo/ROUTES/Route/WORLD/tile.w','world')])
        digest=content.hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory() as external:
            outside=Path(external)
            metadata=outside/'openrailsrs-content.json'
            metadata.write_text(json.dumps({'download_sha256':digest}))
            target=self.root/(self.entry['id']+'-'+digest[:16])
            target.symlink_to(outside,target_is_directory=True)
            with patch.object(content,'emit'),self.assertRaisesRegex(ValueError,'inseguros'):
                content.install(self.entry,self.root,opener=lambda _:Response(payload))
            self.assertEqual(json.loads(metadata.read_text()),{'download_sha256':digest})

    def test_traversal_and_ambiguous_case_are_rejected_before_any_extraction(self):
        for files in [[('../outside', 'bad')], [('C:/outside', 'bad')],
                      [('/outside', 'bad')], [('a/../../bad', 'bad')],
                      [('CAB/file.cvf', 'a'), ('cab/FILE.CVF', 'b')]]:
            with zipfile.ZipFile(io.BytesIO(archive(files))) as z:
                with self.assertRaises(ValueError):
                    content.safe_members(z, 1024)

    def test_symlinks_and_oversized_install_are_rejected(self):
        symlink = zipfile.ZipInfo('route/link')
        symlink.external_attr = 0o120777 << 16
        for data, maximum in [(archive([(symlink, '../outside')]), 1024),
                              (archive([('large', 'x' * 1025)]), 1024)]:
            with zipfile.ZipFile(io.BytesIO(data)) as z:
                with self.assertRaises(ValueError):
                    content.safe_members(z, maximum)

    def test_unicode_texture_names_remain_distinct_without_hiding_case_collisions(self):
        files = [('TEXTURES/DFußballfeld.ace', b'original sharp-s texture'),
                 ('TEXTURES/DFussballfeld.ace', b'original double-s texture')]
        source = self.root / 'textures.zip'
        source.write_bytes(archive(files))
        destination = self.root / 'textures'
        destination.mkdir()
        with patch.object(content, 'emit'):
            content.extract(source, destination, max_install=1024)
        for name, data in files:
            self.assertEqual((destination / name).read_bytes(), data)
        for files in [[('TEXTURES/Ä.ace', 'one'), ('textures/ä.ACE', 'two')],
                      [('CAB/file.cvf', 'one'), ('cab/FILE.CVF', 'two')]]:
            with zipfile.ZipFile(io.BytesIO(archive(files))) as z:
                with self.assertRaises(ValueError):
                    content.safe_members(z, 1024)

    def test_corrupt_or_executable_only_package_does_not_publish(self):
        for data in [b'not a zip', archive([('installer.exe', b'not executed')])]:
            with patch.object(content, 'emit'), self.assertRaises(Exception):
                content.install(self.entry, self.root, opener=lambda _: Response(data))
            self.assertEqual(list(self.root.iterdir()), [])

    def test_cancel_keeps_existing_content(self):
        previous = self.root / 'existing'
        previous.mkdir()
        (previous / 'sentinel').write_text('keep')
        cancel = self.root / 'cancel'
        cancel.touch()
        with patch.object(content, 'emit'), self.assertRaises(InterruptedError):
            content.install(self.entry, self.root, cancel, opener=lambda _: self.fail('no download'))
        self.assertEqual((previous / 'sentinel').read_text(), 'keep')
        self.assertFalse((self.root / ('.' + self.entry['id'] + '.lock')).exists())

    def test_only_curated_free_https_direct_providers_are_automatic(self):
        self.assertEqual(content.provider(self.entry), 'zip')
        for replacement in ['https://example.com/content.zip', 'file:///tmp/content.zip',
                            'https://static.openrails.org:8443/content.zip']:
            self.assertIsNone(content.provider(self.entry | {'url': replacement}))
        self.assertIsNone(content.provider(self.entry | {'compensation': 'commercial'}))

    def test_redirect_is_rejected_before_contacting_another_host_or_http(self):
        contacted = []
        location = 'http://127.0.0.1:9000/internal-action'

        class MemoryNetwork(BaseHandler):
            handler_order = 100
            def https_open(self, request):
                contacted.append(request.full_url)
                headers = Message()
                headers['Location'] = location
                response = addinfourl(io.BytesIO(b''), headers, request.full_url, 302)
                response.msg = 'Found'
                return response
            def http_open(self, request):
                contacted.append(request.full_url)
                raise AssertionError('Redirect must be validated before issuing HTTP')

        for location in ('http://127.0.0.1:9000/internal-action',
                         'https://example.com/untrusted',
                         'https://static.openrails.org:8443/untrusted'):
            contacted.clear()
            with patch.object(content, 'build_opener',
                              lambda *handlers: build_opener(*handlers, MemoryNetwork())):
                with self.assertRaises(ValueError):
                    content.https_open(self.entry['url'])
            self.assertEqual(contacted, [self.entry['url']])

    def test_archive_cannot_publish_importer_manifests_or_prepared_routes(self):
        for name in ('openrailsrs-prepared.json', 'OpenRailsRS-content.json',
                     'nested/openrailsrs-audit.json', 'openrailsrs-import/route/track.toml'):
            with zipfile.ZipFile(io.BytesIO(archive([(name, 'untrusted')]))) as z:
                with self.assertRaises(ValueError):
                    content.safe_members(z, 1024)


if __name__ == '__main__':
    unittest.main()
