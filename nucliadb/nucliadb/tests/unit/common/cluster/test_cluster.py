# Copyright (C) 2021 Bosutech XXI S.L.
#
# nucliadb is offered under the AGPL v3.0 and as commercial software.
# For commercial licensing, contact us at info@nuclia.com.
#
# AGPL:
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as
# published by the Free Software Foundation, either version 3 of the
# License, or (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program. If not, see <http://www.gnu.org/licenses/>.
#

from unittest import mock

import pytest

from nucliadb.common.cluster import manager
from nucliadb.common.cluster.exceptions import NodeClusterSmall
from nucliadb.common.cluster.index_node import IndexNode
from nucliadb.common.cluster.settings import settings


@pytest.fixture(scope="function")
def nodes():
    nodes = {
        "node-0": IndexNode(id="node-0", address="node-0", shard_count=0, dummy=True),
        "node-30": IndexNode(
            id="node-30", address="node-30", shard_count=30, dummy=True
        ),
        "node-40": IndexNode(
            id="node-40", address="node-40", shard_count=40, dummy=True
        ),
    }
    with mock.patch.object(manager, "INDEX_NODES", new=nodes):
        yield nodes


def test_find_nodes_orders_by_shard_count(nodes):
    with mock.patch.object(settings, "node_replicas", 2):
        nodes_found = manager.find_nodes()
        assert len(nodes_found) == settings.node_replicas
        assert nodes_found == ["node-0", "node-30"]


def test_find_nodes_exclude_nodes(nodes):
    with mock.patch.object(settings, "node_replicas", 2):
        excluded_node = "node-0"
        nodes_found = manager.find_nodes(avoid_nodes=[excluded_node])
        assert nodes_found == ["node-30", "node-40"]

        # even if all are used, still should find nodes
        all_nodes = list(nodes.keys())
        assert manager.find_nodes(avoid_nodes=all_nodes) == ["node-0", "node-30"]


def test_find_nodes_raises_error_if_not_enough_nodes_are_found(nodes):
    with mock.patch.object(settings, "node_replicas", 200):
        with pytest.raises(NodeClusterSmall):
            manager.find_nodes()


def test_find_nodes_checks_max_node_replicas_only_if_set(nodes):
    with mock.patch.object(settings, "max_node_replicas", 0):
        with pytest.raises(NodeClusterSmall):
            manager.find_nodes()

    with mock.patch.object(settings, "max_node_replicas", -1):
        assert len(manager.find_nodes())